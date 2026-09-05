//! Regression guardrail (test-only): UI/display string literals must not use
//! symbols outside the app's bundled fonts (Ubuntu + NotoEmoji + Phosphor).
//! Arrows, Greek letters, box drawing, geometric shapes (triangles/squares/
//! circles), dingbat check/cross marks, and single angle quotes render as
//! missing-glyph squares. Use a Phosphor icon (via the icon-button helpers or
//! `Icon::as_str()`, which is a private-use glyph) or plain ASCII instead.
//!
//! Safe and NOT flagged: accented Latin (names), emoji (NotoEmoji covers them),
//! Phosphor private-use glyphs, and common punctuation (middle dot, ellipsis,
//! en/em dash, multiplication sign, degree).

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// Deny-by-default: a glyph is unrenderable unless it is ASCII or on the
    /// explicit safe allow-list — accented Latin (names), a few safe punctuation
    /// marks, Phosphor's private-use glyphs, and emoji (NotoEmoji covers them).
    /// This catches every class the app's fonts can't draw (arrows, Greek, box
    /// drawing, math operators, Cyrillic, …), not just an enumerated few.
    fn unrenderable(ch: char) -> bool {
        let c = ch as u32;
        let safe = c <= 0x7F
            || (0x00A0..=0x024F).contains(&c) // Latin-1 + Latin Extended-A/B (accents)
            || matches!(
                c,
                0x2018 | 0x2019 | 0x201C | 0x201D // curly quotes
                | 0x2013 | 0x2014 // en/em dash
                | 0x2026          // ellipsis
                | 0x00B7          // middle dot
                | 0x00D7          // multiplication sign
                | 0x00B0          // degree
            )
            || (0xE000..=0xF8FF).contains(&c) // Phosphor private-use glyphs
            || (0x2600..=0x26FF).contains(&c) // misc symbols (emoji)
            || (0x1F000..=0x1FAFF).contains(&c); // emoji
        !safe
    }

    /// Report `file:line U+XXXX <ch>` for every unrenderable glyph inside a
    /// string literal on a non-comment line. A naive scanner (good enough here):
    /// it toggles "inside string" on unescaped double quotes and skips lines that
    /// start with `//`.
    fn scan(path: &Path, offenders: &mut Vec<String>) {
        let text = std::fs::read_to_string(path).unwrap();
        for (index, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            let mut in_string = false;
            let mut escaped = false;
            for ch in line.chars() {
                if escaped {
                    escaped = false;
                    continue;
                }
                match ch {
                    '\\' if in_string => escaped = true,
                    '"' => in_string = !in_string,
                    _ if in_string && unrenderable(ch) => offenders.push(format!(
                        "{}:{} U+{:04X} {ch}",
                        path.display(),
                        index + 1,
                        ch as u32
                    )),
                    _ => {}
                }
            }
        }
    }

    fn walk(dir: &Path, offenders: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, offenders);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                scan(&path, offenders);
            }
        }
    }

    #[test]
    fn ui_strings_avoid_unrenderable_glyphs() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        walk(&src, &mut offenders);
        assert!(
            offenders.is_empty(),
            "String literals contain glyphs the app's fonts can't render (use a \
             Phosphor icon or ASCII):\n{}",
            offenders.join("\n")
        );
    }
}
