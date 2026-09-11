//! forge_cef — Forge's CEF offscreen-render helper process.
//!
//! The IDE spawns this helper (as it spawns forge_manager / forge_webview) so
//! CEF's multi-process machinery and its ~150MB Chromium runtime stay out of
//! the main egui binary and a browser crash can't take the IDE down with it.
//! The helper renders a page **windowless** and streams each rendered BGRA
//! frame to the IDE through a memory-mapped file, while reading input and
//! navigation [`Command`]s from its stdin. Both sides of that contract live in
//! the dependency-free `forge-cef-ipc` crate.
//!
//! Usage (spawned by the IDE):
//!   forge_cef --shm <path> --url <url> [--width W --height H]
//! Self-test (build verification on a dev machine — the original OSR probe):
//!   cargo run -p forge-cef --release -- --selftest
//!
//! It cannot be built or run in the Forge dev sandbox (no CEF SDK, no ~150MB
//! Chromium binaries, no display) so it is excluded from CI and built by hand.
//! The `cef` build script downloads the matching CEF distribution on first
//! build (needs CMake + Ninja on PATH); the resulting libcef.dll + resources
//! must sit next to the exe at run time.
//!
//! Note: this is a *console* subsystem binary so `--selftest` output is visible
//! from a terminal. When the IDE spawns it in production it passes
//! CREATE_NO_WINDOW so no console flashes.

use std::sync::{
    atomic::{AtomicU32, Ordering},
    mpsc, Arc,
};

use cef::args::Args;
use cef::*;
use forge_cef_ipc::{self as ipc, Command, KeyKind, MouseButton};
use memmap2::MmapMut;

const DEFAULT_W: u32 = 1024;
const DEFAULT_H: u32 = 768;

/// The offscreen surface the frame buffer is written into, shared (via `Arc`)
/// between the `RenderHandler` (which paints it and reports its size) and the
/// control loop (which resizes it). The map pointer is written only from the
/// CEF UI thread — `on_paint` and the command handlers both run there during
/// `do_message_loop_work` — so there is never concurrent access to the pixels.
struct Surface {
    /// Raw base of the memory-mapped frame buffer and its length. Reconstructed
    /// into a `&mut [u8]` inside `on_paint` for a single-threaded write.
    map_ptr: *mut u8,
    map_len: usize,
    width: AtomicU32,
    height: AtomicU32,
}

// SAFETY: the raw pointer is only dereferenced on the single CEF UI thread
// (on_paint + command application both run under do_message_loop_work on the
// main thread). The `Arc<Surface>` is cloned into the render handler but never
// written from another thread; the stdin reader thread only sends `Command`s
// over a channel and never touches the map. The atomics are safe to read from
// any thread. This is what lets the handle satisfy the wrap-macro's bounds.
unsafe impl Send for Surface {}
unsafe impl Sync for Surface {}

impl Surface {
    fn size(&self) -> (u32, u32) {
        (self.width.load(Ordering::Relaxed), self.height.load(Ordering::Relaxed))
    }
}

fn main() {
    // Lock in the CEF API version before ANY other CEF call, or CEF 152's
    // C-to-C++ layer rejects our structs with "invalid version -1".
    let _ = api_hash(cef::sys::CEF_API_VERSION_LAST, 0);

    let cli = Cli::parse(std::env::args().skip(1));

    // Multi-process bootstrap: CEF re-execs THIS exe for its renderer/GPU/etc.
    // subprocesses. In those, execute_process runs the child loop and returns
    // a non-negative code — exit immediately, before parsing our own args or
    // opening the shared memory (subprocesses have neither).
    let args = Args::new();
    let mut app = make_app();
    let code = execute_process(Some(args.as_main_args()), Some(&mut app), std::ptr::null_mut());
    if code >= 0 {
        std::process::exit(code);
    }

    if cli.selftest {
        run_selftest(&args, &mut app);
        return;
    }

    let Some(shm_path) = cli.shm.clone() else {
        eprintln!("forge_cef: missing --shm <path> (or pass --selftest)");
        std::process::exit(2);
    };

    // Open the IDE-allocated frame-buffer file and map it. The IDE creates and
    // sizes the file and initializes the header before spawning us; we just map
    // and validate it.
    let file = match std::fs::OpenOptions::new().read(true).write(true).open(&shm_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("forge_cef: cannot open shm file {shm_path:?}: {e}");
            std::process::exit(2);
        }
    };
    // SAFETY: the file is a private temp file owned by the IDE for this helper;
    // we accept the standard mmap aliasing caveat that underlies all shared
    // memory here.
    let mut map = match unsafe { MmapMut::map_mut(&file) } {
        Ok(m) => m,
        Err(e) => {
            eprintln!("forge_cef: cannot mmap {shm_path:?}: {e}");
            std::process::exit(2);
        }
    };
    if map.len() < ipc::TOTAL_BYTES || ipc::read_header(&map).is_none() {
        eprintln!("forge_cef: shm file is not a valid Forge CEF frame buffer");
        std::process::exit(2);
    }

    let surface = Arc::new(Surface {
        map_ptr: map.as_mut_ptr(),
        map_len: map.len(),
        width: AtomicU32::new(cli.width.unwrap_or(DEFAULT_W)),
        height: AtomicU32::new(cli.height.unwrap_or(DEFAULT_H)),
    });

    let settings = Settings {
        windowless_rendering_enabled: 1,
        no_sandbox: 1,
        ..Default::default()
    };
    if initialize(Some(args.as_main_args()), Some(&settings), Some(&mut app), std::ptr::null_mut())
        != 1
    {
        eprintln!("forge_cef: cef::initialize failed");
        std::process::exit(1);
    }

    let mut client = make_client(surface.clone());
    let null_parent = WindowInfo::default().parent_window;
    let window_info = WindowInfo::default().set_as_windowless(null_parent);
    let browser_settings = BrowserSettings::default();
    let url = CefString::from(cli.url.as_deref().unwrap_or("about:blank"));

    let Some(browser) = browser_host_create_browser_sync(
        Some(&window_info),
        Some(&mut client),
        Some(&url),
        Some(&browser_settings),
        None,
        None,
    ) else {
        eprintln!("forge_cef: browser_host_create_browser_sync returned null");
        shutdown();
        std::process::exit(1);
    };

    // A background thread reads stdin line-by-line and forwards parsed commands
    // over a channel; the main thread applies them on the CEF UI thread between
    // message-loop pumps (browser host calls must happen on this thread).
    let (tx, rx) = mpsc::channel::<Command>();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(line) => {
                    if let Some(cmd) = Command::parse(&line) {
                        if tx.send(cmd).is_err() {
                            break;
                        }
                    }
                }
                Err(_) => break, // stdin closed → IDE gone
            }
        }
        // stdin closed: tell the main loop to shut down.
        let _ = tx.send(Command::Shutdown);
    });

    println!("forge_cef: ready (windowless, {}x{})", surface.width.load(Ordering::Relaxed), surface.height.load(Ordering::Relaxed));

    let host = browser.host();
    // Take input focus so the page reacts to the events we forward.
    if let Some(h) = &host {
        h.set_focus(1);
    }

    'pump: loop {
        // Apply any pending IDE commands.
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Command::Shutdown => break 'pump,
                Command::Resize { width, height } => {
                    let w = width.clamp(1, ipc::MAX_WIDTH);
                    let h = height.clamp(1, ipc::MAX_HEIGHT);
                    surface.width.store(w, Ordering::Relaxed);
                    surface.height.store(h, Ordering::Relaxed);
                    if let Some(host) = &host {
                        host.was_resized();
                    }
                }
                Command::MouseMove { x, y, modifiers, leaving } => {
                    if let Some(host) = &host {
                        let ev = MouseEvent { x, y, modifiers };
                        host.send_mouse_move_event(Some(&ev), leaving as i32);
                    }
                }
                Command::MouseClick { x, y, modifiers, button, up, click_count } => {
                    if let Some(host) = &host {
                        let ev = MouseEvent { x, y, modifiers };
                        host.send_mouse_click_event(
                            Some(&ev),
                            mouse_button(button),
                            up as i32,
                            click_count,
                        );
                    }
                }
                Command::MouseWheel { x, y, modifiers, delta_x, delta_y } => {
                    if let Some(host) = &host {
                        let ev = MouseEvent { x, y, modifiers };
                        host.send_mouse_wheel_event(Some(&ev), delta_x, delta_y);
                    }
                }
                Command::Key { kind, modifiers, windows_key_code, character } => {
                    if let Some(host) = &host {
                        let ev = KeyEvent {
                            type_: key_kind(kind),
                            modifiers,
                            windows_key_code,
                            native_key_code: 0,
                            character,
                            unmodified_character: character,
                            focus_on_editable_field: 0,
                            ..Default::default()
                        };
                        host.send_key_event(Some(&ev));
                    }
                }
                Command::Focus(on) => {
                    if let Some(host) = &host {
                        host.set_focus(on as i32);
                    }
                }
                Command::Navigate(u) => {
                    if let Some(frame) = browser.main_frame() {
                        frame.load_url(Some(&CefString::from(u.as_str())));
                    }
                }
            }
        }

        do_message_loop_work();
        std::thread::sleep(std::time::Duration::from_millis(4));
    }

    println!("forge_cef: shutting down");
    if let Some(host) = &host {
        host.close_browser(1);
    }
    // Let CEF process the close before shutdown.
    for _ in 0..50 {
        do_message_loop_work();
        std::thread::sleep(std::time::Duration::from_millis(4));
    }
    shutdown();
}

fn mouse_button(b: MouseButton) -> MouseButtonType {
    use cef::sys::cef_mouse_button_type_t as T;
    MouseButtonType::from(match b {
        MouseButton::Left => T::MBT_LEFT,
        MouseButton::Middle => T::MBT_MIDDLE,
        MouseButton::Right => T::MBT_RIGHT,
    })
}

fn key_kind(k: KeyKind) -> KeyEventType {
    use cef::sys::cef_key_event_type_t as T;
    KeyEventType::from(match k {
        KeyKind::RawKeyDown => T::KEYEVENT_RAWKEYDOWN,
        KeyKind::KeyDown => T::KEYEVENT_KEYDOWN,
        KeyKind::KeyUp => T::KEYEVENT_KEYUP,
        KeyKind::Char => T::KEYEVENT_CHAR,
    })
}

/// Do-nothing `App`; the default process/browser handlers are correct for OSR.
fn make_app() -> App {
    wrap_app! {
        struct HelperApp;
        impl App {}
    }
    HelperApp::new()
}

/// A `Client` that returns our OSR `RenderHandler`.
fn make_client(surface: Arc<Surface>) -> Client {
    wrap_client! {
        struct HelperClient {
            handler: RenderHandler,
        }
        impl Client {
            fn render_handler(&self) -> Option<RenderHandler> {
                Some(self.handler.clone())
            }
        }
    }
    HelperClient::new(make_render_handler(surface))
}

/// The OSR `RenderHandler`: `view_rect` reports the current surface size and
/// `on_paint` publishes each BGRA frame into the shared frame buffer.
fn make_render_handler(surface: Arc<Surface>) -> RenderHandler {
    wrap_render_handler! {
        struct HelperRenderHandler {
            surface: Arc<Surface>,
        }
        impl RenderHandler {
            fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
                if let Some(rect) = rect {
                    let (w, h) = self.surface.size();
                    rect.x = 0;
                    rect.y = 0;
                    rect.width = w as i32;
                    rect.height = h as i32;
                }
            }

            fn on_paint(
                &self,
                _browser: Option<&mut Browser>,
                _type: PaintElementType,
                _dirty_rects: Option<&[Rect]>,
                buffer: *const u8,
                width: ::std::os::raw::c_int,
                height: ::std::os::raw::c_int,
            ) {
                if buffer.is_null() || width <= 0 || height <= 0 {
                    return;
                }
                let len = (width as usize) * (height as usize) * 4;
                // SAFETY: CEF guarantees `buffer` points to width*height*4 bytes
                // of BGRA for the duration of this call, and `map_ptr`/`map_len`
                // describe our own mapping. Both accesses are confined to this
                // (the CEF UI) thread, so there is no aliasing with any other.
                unsafe {
                    let src = std::slice::from_raw_parts(buffer, len);
                    let dst = std::slice::from_raw_parts_mut(
                        self.surface.map_ptr,
                        self.surface.map_len,
                    );
                    ipc::publish_frame(dst, width as u32, height as u32, src);
                }
            }
        }
    }
    HelperRenderHandler::new(surface)
}

/// Parsed command line.
struct Cli {
    selftest: bool,
    shm: Option<String>,
    url: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
}

impl Cli {
    fn parse(args: impl Iterator<Item = String>) -> Self {
        let mut cli = Cli { selftest: false, shm: None, url: None, width: None, height: None };
        let mut it = args.peekable();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--selftest" => cli.selftest = true,
                "--shm" => cli.shm = it.next(),
                "--url" => cli.url = it.next(),
                "--width" => cli.width = it.next().and_then(|v| v.parse().ok()),
                "--height" => cli.height = it.next().and_then(|v| v.parse().ok()),
                _ => {} // ignore CEF's own switches
            }
        }
        cli
    }
}

/// Standalone OSR smoke test: init windowless, render a static data: URL, and
/// confirm `on_paint` delivers a framebuffer. This is the original probe, kept
/// so a developer can verify the CEF toolchain end-to-end without the IDE.
fn run_selftest(args: &Args, app: &mut App) {
    let settings = Settings {
        windowless_rendering_enabled: 1,
        no_sandbox: 1,
        ..Default::default()
    };
    if initialize(Some(args.as_main_args()), Some(&settings), Some(app), std::ptr::null_mut()) != 1
    {
        eprintln!("forge_cef --selftest: cef::initialize failed");
        std::process::exit(1);
    }
    println!("forge_cef --selftest: CEF initialized (windowless).");

    let paints = Arc::new(AtomicU32::new(0));
    let mut client = make_selftest_client(paints.clone());
    let null_parent = WindowInfo::default().parent_window;
    let window_info = WindowInfo::default().set_as_windowless(null_parent);
    let browser_settings = BrowserSettings::default();
    let url = CefString::from(
        "data:text/html,<body style='margin:0;background:%23127a3d'><h1 style='color:white;font:48px sans-serif;padding:40px'>Forge CEF OSR selftest</h1></body>",
    );
    if browser_host_create_browser(
        Some(&window_info),
        Some(&mut client),
        Some(&url),
        Some(&browser_settings),
        None,
        None,
    ) != 1
    {
        eprintln!("forge_cef --selftest: create browser failed");
        shutdown();
        std::process::exit(1);
    }

    let start = std::time::Instant::now();
    while start.elapsed().as_secs() < 5 {
        do_message_loop_work();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let n = paints.load(Ordering::Relaxed);
    if n == 0 {
        eprintln!("forge_cef --selftest: NO on_paint callbacks — OSR did not deliver frames.");
    } else {
        println!("forge_cef --selftest: SUCCESS, {n} on_paint callback(s). OSR path works.");
    }
    shutdown();
}

fn make_selftest_client(paints: Arc<AtomicU32>) -> Client {
    wrap_client! {
        struct SelftestClient {
            handler: RenderHandler,
        }
        impl Client {
            fn render_handler(&self) -> Option<RenderHandler> {
                Some(self.handler.clone())
            }
        }
    }
    wrap_render_handler! {
        struct SelftestRender {
            paints: Arc<AtomicU32>,
        }
        impl RenderHandler {
            fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
                if let Some(rect) = rect {
                    rect.x = 0;
                    rect.y = 0;
                    rect.width = DEFAULT_W as i32;
                    rect.height = DEFAULT_H as i32;
                }
            }
            fn on_paint(
                &self,
                _browser: Option<&mut Browser>,
                _type: PaintElementType,
                _dirty_rects: Option<&[Rect]>,
                _buffer: *const u8,
                width: ::std::os::raw::c_int,
                height: ::std::os::raw::c_int,
            ) {
                let n = self.paints.fetch_add(1, Ordering::Relaxed) + 1;
                println!("forge_cef --selftest: on_paint #{n} {width}x{height}");
            }
        }
    }
    SelftestClient::new(SelftestRender::new(paints))
}
