use std::fs;
use std::io::Cursor;
use std::path::Path;

fn render_png(svg: &str, size: u32) -> Vec<u8> {
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(svg.as_bytes(), &opt).expect("parse svg");
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).expect("pixmap");
    let scale = size as f32 / tree.size().width();
    let ts = resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, ts, &mut pixmap.as_mut());
    pixmap.encode_png().expect("encode png")
}

fn write(path: &str, bytes: &[u8]) {
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, bytes).unwrap();
    println!("wrote {path} ({} bytes)", bytes.len());
}

fn main() {
    let repo = std::env::args().nth(1).expect("usage: rasterize <repo_root>");
    let badge = fs::read_to_string(format!("{repo}/assets/logo-badge.svg")).unwrap();
    let mark = fs::read_to_string(format!("{repo}/assets/logo-mark.svg")).unwrap();

    let sizes = [16u32, 32, 48, 64, 128, 256, 512, 1024];

    // PNG icon set (badge) for the installer.
    for &s in &sizes {
        write(
            &format!("{repo}/packaging/icons/icon-{s}.png"),
            &render_png(&badge, s),
        );
    }

    // Windows .ico (multi-size).
    {
        let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
        for &s in &[16u32, 32, 48, 64, 128, 256] {
            let png = render_png(&badge, s);
            let img = ico::IconImage::read_png(Cursor::new(png)).expect("ico read_png");
            dir.add_entry(ico::IconDirEntry::encode(&img).expect("ico encode"));
        }
        let mut buf = Vec::new();
        dir.write(&mut buf).unwrap();
        write(&format!("{repo}/packaging/icons/icon.ico"), &buf);
    }

    // macOS .icns.
    {
        let mut family = icns::IconFamily::new();
        for &s in &[16u32, 32, 64, 128, 256, 512, 1024] {
            let png = render_png(&badge, s);
            if let Ok(image) = icns::Image::read_png(Cursor::new(png)) {
                let _ = family.add_icon(&image);
            }
        }
        let mut buf = Vec::new();
        family.write(&mut buf).unwrap();
        write(&format!("{repo}/packaging/icons/icon.icns"), &buf);
    }

    // App window icon (embedded) + a large badge PNG.
    write(&format!("{repo}/assets/icon-256.png"), &render_png(&badge, 256));

    // Transparent mark PNGs for the splash / README.
    write(&format!("{repo}/assets/mark-256.png"), &render_png(&mark, 256));
    write(&format!("{repo}/assets/mark-512.png"), &render_png(&mark, 512));

    // Site favicon fallback (.ico already written above — copy for the site too).
    write(
        &format!("{repo}/site/favicon-256.png"),
        &render_png(&badge, 256),
    );
}
