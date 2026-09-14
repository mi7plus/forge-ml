# Contributing to Forge ML

Use stable Rust and keep changes scoped to the roadmap. Preserve user files and project credentials. New integrations should prefer subprocess/protocol boundaries and remain optional when they add large native dependencies.

Before submitting a change, run the same gate CI runs. With [`just`](https://github.com/casey/just):

```bash
just ci        # fmt --check, clippy -D warnings (--all-features), build, test
```

Or directly (note the `forge-webview`/`forge-cef` exclusion — those need system
WebKitGTK / the CEF SDK and are linted separately on Windows):

```bash
cargo fmt --all --check
cargo clippy --workspace --exclude forge-webview --exclude forge-cef --all-targets --all-features -- -D warnings
cargo test --workspace --exclude forge-webview --exclude forge-cef --locked
git diff --check
```

Update `ROADMAP.md` only after proportionate verification, and update user documentation for visible behavior. Protocol changes require compatibility tests. UI changes should remain keyboard reachable, include text or symbols rather than relying on color alone, and be checked in light, dark, high-contrast, and reduced-motion modes.

For icons and symbols in the UI, use the bundled Phosphor icons (via the `ui::theme` icon-button helpers or `Icon::as_str()`), not raw Unicode arrows/triangles/Greek/dingbats — those aren't in the app's fonts and render as squares. The `glyph_guard` test enforces this over `src`.

Never trigger package publication from tests or ordinary UI discovery. Publishing and update installation must remain explicit operations.
