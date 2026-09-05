# Contributing to Forge ML

Use stable Rust and keep changes scoped to the roadmap. Preserve user files and project credentials. New integrations should prefer subprocess/protocol boundaries and remain optional when they add large native dependencies.

Before submitting a change, run:

```powershell
cargo fmt --all --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --features millwright,adbc --all-targets -- -D warnings
git diff --check
```

Update `ROADMAP.md` only after proportionate verification, and update user documentation for visible behavior. Protocol changes require compatibility tests. UI changes should remain keyboard reachable, include text or symbols rather than relying on color alone, and be checked in light, dark, high-contrast, and reduced-motion modes.

For icons and symbols in the UI, use the bundled Phosphor icons (via the `ui::theme` icon-button helpers or `Icon::as_str()`), not raw Unicode arrows/triangles/Greek/dingbats — those aren't in the app's fonts and render as squares. The `glyph_guard` test enforces this over `src`.

Never trigger package publication from tests or ordinary UI discovery. Publishing and update installation must remain explicit operations.
