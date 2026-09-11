//! forge_cef_probe — a standalone smoke test for CEF (Chromium Embedded
//! Framework) *offscreen* rendering, the path Forge needs to embed a real
//! Chromium surface inside an egui pane.
//!
//! This binary deliberately does NOT touch egui. It proves the hard part in
//! isolation: that the `cef` binding builds against the downloaded CEF binary
//! distribution on this machine, that CEF initializes in windowless mode, and
//! that the `on_paint` callback delivers a BGRA framebuffer we could hand to a
//! GPU texture. Once this prints paint callbacks with sane dimensions, wiring
//! the buffer into an egui `TextureHandle` in the IDE is mechanical.
//!
//! It cannot be built or run inside the Forge dev sandbox (no CEF SDK / no
//! ~150 MB Chromium binaries, and CEF needs a real windowing environment), so
//! it is excluded from CI and is built + run by hand on a developer machine:
//!
//!   cargo run -p forge-cef --release
//!
//! The `cef` crate's build script downloads the matching CEF distribution the
//! first time (or honours `CEF_PATH` pointing at an existing one). On Windows
//! the resulting `libcef.dll` + resources must sit next to the built exe; on
//! Linux they must be on the loader path. See the crate README for details.
//!
//! Expected output on success: a handful of lines like
//!   [forge-cef] on_paint #1  type=View  1024x768  (3145728 bytes BGRA)
//! followed by a clean shutdown.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

// Glob import: the `wrap_app!` / `wrap_client!` / `wrap_render_handler!` macros
// expand to `impl WrapApp / ImplApp / WrapClient / ImplClient / ...` referring
// to those traits by bare name, so every one of them must be in scope at the
// expansion site. The glob is the crate's intended usage (its bindings are a
// flat re-export designed for it). `Args` is the one item under a submodule.
use cef::args::Args;
use cef::*;

/// The page CEF renders. A `data:` URL keeps the probe fully offline — no
/// network, no local file staging — so a green box + heading is all we need to
/// confirm pixels are flowing through `on_paint`.
const PROBE_URL: &str =
    "data:text/html,<body style='margin:0;background:%23127a3d'><h1 style='color:white;font:48px sans-serif;padding:40px'>Forge CEF OSR probe</h1></body>";

const VIEW_W: i32 = 1024;
const VIEW_H: i32 = 768;

fn main() {
    // 0. Lock in the CEF API version. CEF 152 is versioned: until the first
    //    `cef_api_hash` call stamps a version, `cef_api_version()` returns -1
    //    and CEF's C-to-C++ layer rejects any struct we hand it with
    //    "CefApp_0_CToCpp called with invalid version -1". This MUST run before
    //    any App/Client is built or `execute_process` is called. We request the
    //    same version the wrapper was compiled against (CEF_API_VERSION_LAST =
    //    15200 here).
    let _ = api_hash(cef::sys::CEF_API_VERSION_LAST, 0);

    // 1. Multi-process bootstrap. CEF re-executes THIS exe for its helper
    //    subprocesses (renderer, GPU, utility, …). In those subprocesses
    //    `execute_process` runs the child's message loop and returns a non-
    //    negative exit code; we must exit immediately and NOT fall through to
    //    `initialize`. Only the real browser process gets -1 here.
    let args = Args::new();
    let mut app = make_app();
    let code = execute_process(
        Some(args.as_main_args()),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    if code >= 0 {
        std::process::exit(code);
    }

    // 2. Initialize CEF in windowless (offscreen) mode. `no_sandbox` keeps the
    //    probe simple (the real IDE integration can revisit the sandbox).
    let settings = Settings {
        windowless_rendering_enabled: 1,
        no_sandbox: 1,
        ..Default::default()
    };
    let ok = initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    if ok != 1 {
        eprintln!("[forge-cef] cef::initialize failed (returned {ok})");
        std::process::exit(1);
    }
    println!("[forge-cef] CEF initialized (windowless). Creating offscreen browser…");

    // 3. Create the windowless browser. `set_as_windowless` selects the ALLOY
    //    runtime (required for OSR) and flags windowless rendering. We take a
    //    null parent window handle straight off a default `WindowInfo` so we
    //    never have to name the per-platform handle type.
    let paints = Arc::new(AtomicU64::new(0));
    let mut client = make_client(paints.clone());
    let null_parent = WindowInfo::default().parent_window;
    let window_info = WindowInfo::default().set_as_windowless(null_parent);
    let browser_settings = BrowserSettings::default();
    let url = CefString::from(PROBE_URL);

    let created = browser_host_create_browser(
        Some(&window_info),
        Some(&mut client),
        Some(&url),
        Some(&browser_settings),
        None,
        None,
    );
    if created != 1 {
        eprintln!("[forge-cef] browser_host_create_browser failed (returned {created})");
        shutdown();
        std::process::exit(1);
    }

    // 4. Pump the CEF message loop by hand (the same way the IDE will, from its
    //    egui frame callback) and wait for paints to arrive. Give it a few
    //    seconds of wall-clock time, then report and shut down.
    println!("[forge-cef] pumping message loop, waiting for on_paint…");
    let start = std::time::Instant::now();
    while start.elapsed().as_secs() < 5 {
        do_message_loop_work();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    let total = paints.load(Ordering::Relaxed);
    if total == 0 {
        eprintln!(
            "[forge-cef] NO on_paint callbacks received — OSR did not deliver frames. \
             Check that a GPU/software compositor is available and libcef resources are present."
        );
    } else {
        println!("[forge-cef] SUCCESS: received {total} on_paint callback(s). OSR path works.");
    }

    // 5. Orderly shutdown so CEF flushes its subprocesses.
    println!("[forge-cef] shutting down CEF…");
    shutdown();
}

/// A do-nothing `App`. The probe needs no custom process/browser handlers; the
/// default trait impls are correct. It exists only because `execute_process`
/// and `initialize` both take an `App`.
fn make_app() -> App {
    wrap_app! {
        struct ProbeApp;
        impl App {}
    }
    ProbeApp::new()
}

/// A `Client` whose only job is to hand CEF our `RenderHandler`, which is what
/// makes the browser render offscreen. The paint counter is threaded through so
/// `on_paint` can tick it.
fn make_client(paints: Arc<AtomicU64>) -> Client {
    wrap_client! {
        struct ProbeClient {
            handler: RenderHandler,
        }
        impl Client {
            fn render_handler(&self) -> Option<RenderHandler> {
                Some(self.handler.clone())
            }
        }
    }
    ProbeClient::new(make_render_handler(paints))
}

/// The `RenderHandler` is the heart of OSR. `view_rect` tells CEF the offscreen
/// surface size (it must be non-zero or CEF never paints), and `on_paint`
/// receives each dirty BGRA framebuffer — exactly the bytes the IDE will upload
/// to an egui texture.
fn make_render_handler(paints: Arc<AtomicU64>) -> RenderHandler {
    wrap_render_handler! {
        struct ProbeRenderHandler {
            paints: Arc<AtomicU64>,
        }
        impl RenderHandler {
            fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
                if let Some(rect) = rect {
                    rect.x = 0;
                    rect.y = 0;
                    rect.width = VIEW_W;
                    rect.height = VIEW_H;
                }
            }

            fn on_paint(
                &self,
                _browser: Option<&mut Browser>,
                type_: PaintElementType,
                _dirty_rects: Option<&[Rect]>,
                _buffer: *const u8,
                width: ::std::os::raw::c_int,
                height: ::std::os::raw::c_int,
            ) {
                let n = self.paints.fetch_add(1, Ordering::Relaxed) + 1;
                // BGRA => 4 bytes per pixel.
                let bytes = (width as i64) * (height as i64) * 4;
                println!(
                    "[forge-cef] on_paint #{n}  type={type_:?}  {width}x{height}  ({bytes} bytes BGRA)"
                );
            }
        }
    }
    ProbeRenderHandler::new(paints)
}
