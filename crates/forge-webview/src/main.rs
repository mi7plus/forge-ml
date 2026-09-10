//! Forge WebView — a minimal window that renders an HTML file (or URL) with a
//! real engine (WebView2 / WebKit) inside the app, launched by the IDE's HTML
//! preview so users get full CSS/JS rather than an external browser. Kept as its
//! own binary so the heavy WebView deps and their event loop stay out of the
//! main egui app.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use wry::WebViewBuilder;

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_default();
    if arg.is_empty() {
        eprintln!("usage: forge_webview <html-file-or-url>");
        return;
    }
    let url = to_url(&arg);

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("Forge Preview")
        .with_inner_size(tao::dpi::LogicalSize::new(1024.0, 800.0))
        .build(&event_loop)
        .expect("create window");

    let _webview = WebViewBuilder::new()
        .with_url(&url)
        .build(&window)
        .expect("create webview");

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            *control_flow = ControlFlow::Exit;
        }
    });
}

/// Turn a CLI argument into a URL the WebView can load: pass URLs through, and
/// convert a local path to a `file://` URL.
fn to_url(arg: &str) -> String {
    if arg.starts_with("http://") || arg.starts_with("https://") || arg.starts_with("file://") {
        return arg.to_owned();
    }
    let absolute = std::fs::canonicalize(arg)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| arg.to_owned());
    let cleaned = absolute
        .trim_start_matches(r"\\?\")
        .replace('\\', "/");
    format!("file:///{cleaned}")
}
