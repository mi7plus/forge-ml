//! Shared contract between the Forge IDE and the `forge_cef` offscreen-render
//! helper. Two independent processes talk through:
//!
//!   * a **memory-mapped file** carrying the rendered BGRA framebuffer, laid
//!     out as [`Header`] + two pixel slots and updated with a lock-free
//!     double-buffer flip (the helper writes, the IDE reads, no tearing); and
//!   * a **line-oriented command channel** (the helper's stdin) carrying input
//!     and navigation events from the IDE to the browser ([`Command`]).
//!
//! This crate is deliberately `std`-only and has NO dependency on `cef`, so the
//! IDE can link it to read frames and format commands without dragging in
//! CEF's ~150MB Chromium download. The helper binary (`forge-cef`) depends on
//! both this crate and `cef`.
//!
//! ## Frame transport (double buffering)
//!
//! The map is `HEADER_BYTES + 2 * SLOT_BYTES`. Two slots let the writer fill
//! the *inactive* slot while the reader reads the *active* one, then publish by
//! flipping `active` and bumping `frame_seq` (both `AtomicU32` living in the
//! shared bytes). Because the writer only ever touches `1 - active` and the
//! reader only ever touches `active`, they never share a slot — so the reader
//! sees a whole frame or the previous whole frame, never a half-written one.

use std::sync::atomic::{AtomicU32, Ordering};

/// `"FCEF"` little-endian — sanity check that the IDE mapped the right file.
pub const MAGIC: u32 = 0x4645_4346;
/// Bump if the on-disk layout below ever changes incompatibly.
pub const LAYOUT_VERSION: u32 = 1;

/// Maximum offscreen surface we allocate slots for. The painted region is
/// `width * height` (from the header) and is usually far smaller; slots are
/// sized to the max so a resize never has to remap the file cross-process.
/// 3840x2160 (4K) * 4 bytes = ~33 MB per slot, ~66 MB total — virtual address
/// space backed by a temp file, so only touched pages actually materialize.
pub const MAX_WIDTH: u32 = 3840;
pub const MAX_HEIGHT: u32 = 2160;

/// Bytes in one pixel slot (BGRA, 4 bytes/pixel).
pub const SLOT_BYTES: usize = (MAX_WIDTH as usize) * (MAX_HEIGHT as usize) * 4;
/// Fixed header size in bytes (also the offset of slot 0).
pub const HEADER_BYTES: usize = 64;
/// Total mapped-file size the IDE must allocate and the helper opens.
pub const TOTAL_BYTES: usize = HEADER_BYTES + 2 * SLOT_BYTES;

// Byte offsets of the header fields (each a u32, 4-byte aligned). The mmap base
// is page-aligned, so every field is correctly aligned for atomic access.
const OFF_MAGIC: usize = 0;
const OFF_VERSION: usize = 4;
const OFF_MAX_WIDTH: usize = 8;
const OFF_MAX_HEIGHT: usize = 12;
const OFF_ACTIVE: usize = 16; // atomic: 0 or 1, which slot holds the latest frame
const OFF_FRAME_SEQ: usize = 20; // atomic: increments on each publish
const OFF_WIDTH: usize = 24; // painted width of the active slot
const OFF_HEIGHT: usize = 28; // painted height of the active slot

/// A decoded snapshot of the header's plain (non-atomic) fields.
#[derive(Clone, Copy, Debug)]
pub struct Header {
    pub magic: u32,
    pub version: u32,
    pub max_width: u32,
    pub max_height: u32,
}

#[inline]
fn atomic_at(buf: &[u8], off: usize) -> &AtomicU32 {
    // SAFETY: `off` is a compile-time-known 4-byte-aligned offset within the
    // header, the buffer is at least HEADER_BYTES long (callers guarantee), and
    // the mmap base is page-aligned so the address is aligned for AtomicU32.
    unsafe { AtomicU32::from_ptr(buf.as_ptr().add(off) as *mut u32) }
}

#[inline]
fn read_u32(buf: &[u8], off: usize) -> u32 {
    u32::from_ne_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}

#[inline]
fn write_u32(buf: &mut [u8], off: usize, val: u32) {
    buf[off..off + 4].copy_from_slice(&val.to_ne_bytes());
}

/// Initialize the header of a freshly created map. Called once by whoever
/// allocates the file (the IDE, or the helper in self-test mode).
pub fn init_header(buf: &mut [u8]) {
    assert!(buf.len() >= TOTAL_BYTES, "map too small for CEF frame buffer");
    write_u32(buf, OFF_MAGIC, MAGIC);
    write_u32(buf, OFF_VERSION, LAYOUT_VERSION);
    write_u32(buf, OFF_MAX_WIDTH, MAX_WIDTH);
    write_u32(buf, OFF_MAX_HEIGHT, MAX_HEIGHT);
    atomic_at(buf, OFF_ACTIVE).store(0, Ordering::Relaxed);
    atomic_at(buf, OFF_FRAME_SEQ).store(0, Ordering::Relaxed);
    write_u32(buf, OFF_WIDTH, 0);
    write_u32(buf, OFF_HEIGHT, 0);
}

/// Read and validate the header. Returns `None` if the magic/version/size don't
/// match — i.e. the map isn't a Forge CEF frame buffer we understand.
pub fn read_header(buf: &[u8]) -> Option<Header> {
    if buf.len() < HEADER_BYTES {
        return None;
    }
    let h = Header {
        magic: read_u32(buf, OFF_MAGIC),
        version: read_u32(buf, OFF_VERSION),
        max_width: read_u32(buf, OFF_MAX_WIDTH),
        max_height: read_u32(buf, OFF_MAX_HEIGHT),
    };
    if h.magic != MAGIC || h.version != LAYOUT_VERSION {
        return None;
    }
    Some(h)
}

#[inline]
fn slot_offset(index: u32) -> usize {
    HEADER_BYTES + (index as usize & 1) * SLOT_BYTES
}

/// Publish a rendered frame (helper side). Writes `bgra` (`width*height*4`
/// bytes, top-to-bottom) into the currently inactive slot, records its
/// dimensions, then atomically flips `active` and bumps `frame_seq` so the
/// reader picks it up. `bgra` shorter than `width*height*4` is ignored (a
/// defensive no-op) rather than risking an out-of-bounds copy.
pub fn publish_frame(buf: &mut [u8], width: u32, height: u32, bgra: &[u8]) {
    if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT {
        return;
    }
    let needed = (width as usize) * (height as usize) * 4;
    if bgra.len() < needed || buf.len() < TOTAL_BYTES {
        return;
    }
    let active = atomic_at(buf, OFF_ACTIVE).load(Ordering::Acquire);
    let target = 1 - (active & 1);
    let off = slot_offset(target);
    buf[off..off + needed].copy_from_slice(&bgra[..needed]);
    // Record dimensions of the frame we just wrote, then publish with a release
    // flip so the reader that sees the new `active`/`frame_seq` also sees the
    // matching width/height and pixels.
    write_u32(buf, OFF_WIDTH, width);
    write_u32(buf, OFF_HEIGHT, height);
    atomic_at(buf, OFF_ACTIVE).store(target, Ordering::Release);
    atomic_at(buf, OFF_FRAME_SEQ).fetch_add(1, Ordering::Release);
}

/// A frame handed to the reader: its publish sequence number, dimensions, and a
/// borrow of the active slot's pixel bytes (`width*height*4`, BGRA).
pub struct FrameRef<'a> {
    pub seq: u32,
    pub width: u32,
    pub height: u32,
    pub bgra: &'a [u8],
}

/// Read the latest published frame (IDE side) if it is newer than `last_seq`.
/// Returns `None` when no new frame has been published since `last_seq`, or the
/// map holds nothing valid yet. Pass the returned `seq` back as `last_seq` next
/// time so the IDE only re-uploads a texture when the frame actually changed.
pub fn latest_frame(buf: &[u8], last_seq: u32) -> Option<FrameRef<'_>> {
    if buf.len() < TOTAL_BYTES {
        return None;
    }
    let seq = atomic_at(buf, OFF_FRAME_SEQ).load(Ordering::Acquire);
    if seq == 0 || seq == last_seq {
        return None;
    }
    let active = atomic_at(buf, OFF_ACTIVE).load(Ordering::Acquire);
    let width = read_u32(buf, OFF_WIDTH);
    let height = read_u32(buf, OFF_HEIGHT);
    if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT {
        return None;
    }
    let needed = (width as usize) * (height as usize) * 4;
    let off = slot_offset(active);
    Some(FrameRef {
        seq,
        width,
        height,
        bgra: &buf[off..off + needed],
    })
}

/// Mouse buttons, matching CEF's `cef_mouse_button_type_t` ordering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left = 0,
    Middle = 1,
    Right = 2,
}

impl MouseButton {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::Left),
            1 => Some(Self::Middle),
            2 => Some(Self::Right),
            _ => None,
        }
    }
}

/// Key event phase, matching CEF's `cef_key_event_type_t` ordering
/// (RAWKEYDOWN=0, KEYDOWN=1, KEYUP=2, CHAR=3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    RawKeyDown = 0,
    KeyDown = 1,
    KeyUp = 2,
    Char = 3,
}

impl KeyKind {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::RawKeyDown),
            1 => Some(Self::KeyDown),
            2 => Some(Self::KeyUp),
            3 => Some(Self::Char),
            _ => None,
        }
    }
}

/// Commands sent from the IDE to the helper over the helper's stdin, one per
/// line. Coordinates are in device-independent pixels within the offscreen
/// surface (top-left origin), matching what the helper reports via `view_rect`.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Resize the offscreen surface to `width`x`height` **logical** points and
    /// repaint. `device_scale` is the display's pixels-per-point: the helper
    /// renders the offscreen buffer at `width*scale` x `height*scale` physical
    /// pixels (crisp on hi-DPI) while input and layout stay in logical points.
    Resize { width: u32, height: u32, device_scale: f32 },
    /// Pointer moved to (x, y). `modifiers` is CEF's event-flags bitmask.
    MouseMove { x: i32, y: i32, modifiers: u32, leaving: bool },
    /// A mouse button went down (`up=false`) or up (`up=true`).
    MouseClick {
        x: i32,
        y: i32,
        modifiers: u32,
        button: MouseButton,
        up: bool,
        click_count: i32,
    },
    /// Scroll wheel at (x, y) by (delta_x, delta_y) pixels.
    MouseWheel {
        x: i32,
        y: i32,
        modifiers: u32,
        delta_x: i32,
        delta_y: i32,
    },
    /// A key event. `windows_key_code` is the VK code; `character` the UTF-16
    /// code unit for `Char` events (0 otherwise).
    Key {
        kind: KeyKind,
        modifiers: u32,
        windows_key_code: i32,
        character: u16,
    },
    /// Give (`true`) or take (`false`) input focus.
    Focus(bool),
    /// Navigate the main frame to `url`.
    Navigate(String),
    /// Cleanly close the browser and exit the helper.
    Shutdown,
}

impl Command {
    /// Serialize to a single line (no trailing newline). `Navigate`'s URL is
    /// placed last and takes the rest of the line, so URLs may contain spaces.
    pub fn to_line(&self) -> String {
        match self {
            Command::Resize { width, height, device_scale } => {
                format!("resize {width} {height} {device_scale}")
            }
            Command::MouseMove { x, y, modifiers, leaving } => {
                format!("mouse_move {x} {y} {modifiers} {}", *leaving as u8)
            }
            Command::MouseClick { x, y, modifiers, button, up, click_count } => format!(
                "mouse_click {x} {y} {modifiers} {} {} {click_count}",
                *button as u32, *up as u8
            ),
            Command::MouseWheel { x, y, modifiers, delta_x, delta_y } => {
                format!("mouse_wheel {x} {y} {modifiers} {delta_x} {delta_y}")
            }
            Command::Key { kind, modifiers, windows_key_code, character } => {
                format!("key {} {modifiers} {windows_key_code} {character}", *kind as u32)
            }
            Command::Focus(on) => format!("focus {}", *on as u8),
            Command::Navigate(url) => format!("navigate {url}"),
            Command::Shutdown => "shutdown".to_string(),
        }
    }

    /// Parse one line produced by [`Command::to_line`]. Returns `None` on any
    /// malformed input (the helper simply ignores unparseable lines).
    pub fn parse(line: &str) -> Option<Command> {
        let line = line.trim_end_matches(['\r', '\n']);
        let mut it = line.splitn(2, ' ');
        let verb = it.next()?;
        let rest = it.next().unwrap_or("");
        let mut f = rest.split_whitespace();
        match verb {
            "resize" => Some(Command::Resize {
                width: f.next()?.parse().ok()?,
                height: f.next()?.parse().ok()?,
                // Optional for backward compatibility; default to 1.0.
                device_scale: f.next().and_then(|s| s.parse().ok()).unwrap_or(1.0),
            }),
            "mouse_move" => Some(Command::MouseMove {
                x: f.next()?.parse().ok()?,
                y: f.next()?.parse().ok()?,
                modifiers: f.next()?.parse().ok()?,
                leaving: f.next().map(|s| s != "0").unwrap_or(false),
            }),
            "mouse_click" => Some(Command::MouseClick {
                x: f.next()?.parse().ok()?,
                y: f.next()?.parse().ok()?,
                modifiers: f.next()?.parse().ok()?,
                button: MouseButton::from_u32(f.next()?.parse().ok()?)?,
                up: f.next()? != "0",
                click_count: f.next()?.parse().ok()?,
            }),
            "mouse_wheel" => Some(Command::MouseWheel {
                x: f.next()?.parse().ok()?,
                y: f.next()?.parse().ok()?,
                modifiers: f.next()?.parse().ok()?,
                delta_x: f.next()?.parse().ok()?,
                delta_y: f.next()?.parse().ok()?,
            }),
            "key" => Some(Command::Key {
                kind: KeyKind::from_u32(f.next()?.parse().ok()?)?,
                modifiers: f.next()?.parse().ok()?,
                windows_key_code: f.next()?.parse().ok()?,
                character: f.next()?.parse().ok()?,
            }),
            "focus" => Some(Command::Focus(f.next()? != "0")),
            // URL is everything after "navigate " verbatim (may contain spaces).
            "navigate" => {
                if rest.is_empty() {
                    None
                } else {
                    Some(Command::Navigate(rest.to_string()))
                }
            }
            "shutdown" => Some(Command::Shutdown),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `TOTAL_BYTES` buffer guaranteed 4-byte aligned (backed by `Vec<u32>`),
    /// matching the page-aligned mmap the real code always uses — `atomic_at`
    /// requires 4-byte alignment for `AtomicU32::from_ptr`.
    struct Aligned(Vec<u32>);
    impl Aligned {
        fn new() -> Self {
            Aligned(vec![0u32; TOTAL_BYTES / 4])
        }
        fn bytes(&mut self) -> &mut [u8] {
            // SAFETY: Vec<u32> is 4-aligned; TOTAL_BYTES is a multiple of 4.
            unsafe { std::slice::from_raw_parts_mut(self.0.as_mut_ptr() as *mut u8, TOTAL_BYTES) }
        }
    }

    #[test]
    fn header_roundtrip() {
        let mut backing = Aligned::new();
        let buf = backing.bytes();
        init_header(buf);
        let h = read_header(buf).expect("valid header");
        assert_eq!(h.magic, MAGIC);
        assert_eq!(h.version, LAYOUT_VERSION);
        assert_eq!(h.max_width, MAX_WIDTH);
        assert_eq!(h.max_height, MAX_HEIGHT);
        // Nothing published yet.
        assert!(latest_frame(buf, 0).is_none());
    }

    #[test]
    fn publish_then_read_double_buffers() {
        let mut backing = Aligned::new();
        let buf = backing.bytes();
        init_header(buf);

        let frame_a = vec![0xAAu8; 2 * 2 * 4];
        publish_frame(buf, 2, 2, &frame_a);
        let f = latest_frame(buf, 0).expect("frame A");
        assert_eq!((f.width, f.height), (2, 2));
        assert_eq!(f.seq, 1);
        assert!(f.bgra.iter().all(|&b| b == 0xAA));
        let seq_a = f.seq;

        // No new frame since seq_a.
        assert!(latest_frame(buf, seq_a).is_none());

        // Second publish must land in the *other* slot and be readable.
        let frame_b = vec![0xBBu8; 3 * 4];
        publish_frame(buf, 3, 1, &frame_b);
        let f = latest_frame(buf, seq_a).expect("frame B");
        assert_eq!((f.width, f.height), (3, 1));
        assert_eq!(f.seq, 2);
        assert!(f.bgra.iter().all(|&b| b == 0xBB));
    }

    #[test]
    fn publish_rejects_oversize_and_short_buffers() {
        let mut backing = Aligned::new();
        let buf = backing.bytes();
        init_header(buf);
        // Oversize dims: ignored.
        publish_frame(buf, MAX_WIDTH + 1, 1, &[0u8; 8]);
        assert!(latest_frame(buf, 0).is_none());
        // Short source buffer: ignored.
        publish_frame(buf, 4, 4, &[0u8; 4]);
        assert!(latest_frame(buf, 0).is_none());
    }

    #[test]
    fn command_line_roundtrip() {
        let cases = [
            Command::Resize { width: 800, height: 600, device_scale: 1.5 },
            Command::MouseMove { x: 10, y: -5, modifiers: 4, leaving: true },
            Command::MouseClick {
                x: 1,
                y: 2,
                modifiers: 0,
                button: MouseButton::Right,
                up: true,
                click_count: 2,
            },
            Command::MouseWheel { x: 3, y: 4, modifiers: 0, delta_x: 0, delta_y: -120 },
            Command::Key {
                kind: KeyKind::Char,
                modifiers: 2,
                windows_key_code: 65,
                character: 0x41,
            },
            Command::Focus(true),
            Command::Navigate("https://example.com/a b?x=1".to_string()),
            Command::Shutdown,
        ];
        for c in cases {
            let line = c.to_line();
            let parsed = Command::parse(&line).expect("parse");
            assert_eq!(parsed, c, "line was {line:?}");
        }
    }

    #[test]
    fn parse_ignores_garbage() {
        assert!(Command::parse("").is_none());
        assert!(Command::parse("bogus 1 2 3").is_none());
        assert!(Command::parse("resize notanumber 5").is_none());
        assert!(Command::parse("navigate").is_none());
    }
}
