// Embed the Forge ML icon (and version metadata) into the Windows executable so
// Explorer, the Start-menu shortcut, and the taskbar show the app icon rather
// than the default Rust binary icon. No-op on other platforms.
fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("packaging/icons/icon.ico");
        res.set("ProductName", "Forge ML");
        res.set("FileDescription", "Forge ML");
        if let Err(error) = res.compile() {
            // Don't fail the build if the resource compiler is unavailable; the
            // app still runs, just without the embedded icon.
            println!("cargo:warning=could not embed Windows icon: {error}");
        }
    }
}
