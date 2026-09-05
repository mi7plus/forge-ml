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

    /// True for codepoint ranges the app's fonts do not cover, so a glyph there
    /// renders as a square. Kept deliberately narrow to avoid false positives.
    fn unrenderable(ch: char) -> bool {
        matches!(ch as u32,
            0x0370..=0x03FF   // Greek (mu, sigma, ...)
            | 0x2039 | 0x203A // single angle quotation marks
            | 0x2190..=0x21FF // arrows
            | 0x2500..=0x257F // box drawing
            | 0x25A0..=0x25FF // geometric shapes (triangles, squares, circles)
            | 0x2700..=0x27BF // dingbats (check/cross marks)
            | 0x27F0..=0x27FF // supplemental arrows-A
            | 0x2B00..=0x2BFF // misc symbols and arrows
        )
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
