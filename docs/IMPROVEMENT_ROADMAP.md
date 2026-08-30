# Forge ML — Improvement Roadmap

Status: proposed (2026-08-30). A prioritized plan for taking Forge ML from a
capable prototype to a complete Rust ML IDE. Grounded in the current codebase;
each item notes what, why, where, and rough effort.

Related: [UX improvement plan](UX_IMPROVEMENT_PLAN.md) (the dockable-workspace
work, now shipped).

## Where the codebase stands

Already strong: a fully dockable, persistent `egui_tiles` workspace; multiple
editor tabs with `rust-analyzer` (diagnostics, completion, hover, go-to-def);
`//# %%` notebook cells on a persistent Evcxr runtime; multiple embedded
terminals and multiple independent Rust kernels; data import (CSV/TSV/JSON
Lines/Parquet/Arrow) with a virtualized grid and quality profiles; 11 plot
families; experiments/runs with SHA-256 provenance; a model registry, generated
Axum inference services, and deployment/drift monitoring; embedded Burn training
and Millwright ONNX export; a read-only SQL workbench; object storage; remote
Jupyter; Git/GitHub/packages panes; off-by-default diagnostics; and 132 tests.

## The biggest gaps (verified)

- **ML depth:** [`deep_learning.rs`](../src/deep_learning.rs) only implements
  native **linear regression**. No classification, no MLP, no cross-entropy /
  accuracy. This is the largest gap for an *ML* IDE.
- **Editor is read-mostly:** the LSP client
  ([`lsp.rs`](../src/lsp.rs)) requests only completion, hover, definition, and
  diagnostics — **no format, rename, find-references, code actions, or signature
  help**. No **rustfmt** or **clippy** anywhere in the tree.
- **No PR CI:** `.github/workflows/` has only `pages.yml` and `release.yml`;
  nothing builds or runs the 132 tests on pull requests.
- **No cargo task UI:** the only cargo action is `cargo check` ("Run code
  analysis"). No build / test / run / bench, no test explorer.
- **Maintainability:** [`main.rs`](../src/main.rs) is ~9,800 lines; every feature
  now touches one file.

---

## Tier 1 — Quick, high-leverage wins (days)

1. **rustfmt integration.** Add *Format Document* and optional format-on-save via
   `rustfmt --emit stdout` on the background integration worker. Low effort,
   high daily value.
2. **clippy in Problems.** Run `cargo clippy --message-format=json` and merge
   lints into the existing Problems pane (the cargo-output parsing already
   exists for `cargo check`).
3. **PR CI workflow.** `.github/workflows/ci.yml`: `cargo build`, `cargo test`,
   `cargo clippy -- -D warnings`, `cargo fmt --check` on pull requests. The tests
   exist; nothing currently gates regressions.
4. **Cargo tasks pane/menu.** Build / Test / Run / Bench / Clippy with streamed
   output (reuse a terminal or a jobs pane). Add a minimal **test explorer** by
   parsing `cargo test -- --list`.
5. **Site refresh.** The homepage and guide under [`site/`](../site) do not yet
   mention the **terminals** or **Rust kernels**; add them, plus the
   *New terminal / New Rust kernel* actions to the guide's shortcuts.

## Tier 2 — Editor & LSP depth (1–2 weeks)

6. **rust-analyzer feature set:** rename, find-references, code actions /
   quick-fixes, and signature help. The client scaffolding in
   [`lsp.rs`](../src/lsp.rs) and the request plumbing in
   [`main.rs`](../src/main.rs) already exist; this extends them.
7. **Snippet support** (`snippetSupport` is currently hard-coded to `false`).
8. **Command palette upgrade:** fuzzy matching, recent commands, and
   jump-to-symbol (the outline already parses symbols).
9. **Welcome / onboarding.** Turn the existing `welcome_tab()` into a real start
   screen: open-recent, an example-gallery launcher (the
   [`examples/`](../examples) already exist), and rust-analyzer install status.
10. **Customizable keyboard shortcuts (Settings page).** Add a *Keyboard* section
    to the existing Settings window (`settings_window()` in
    [`main.rs`](../src/main.rs)) that lists every command with its current
    binding and lets the user rebind it. Concretely:
    - **Introduce a keymap:** a `HashMap<Command, Shortcut>` (or a
      `Vec<(Action, egui::KeyboardShortcut)>`) that replaces today's hard-coded
      checks in `accessibility_shortcuts()` and the `ui.input(...)` blocks at the
      top of `fn ui` (Ctrl+S, Ctrl+N, Ctrl+F, Ctrl+Shift+F, Ctrl+Space, Shift+
      Enter, Ctrl+Shift+Enter, F6, Ctrl+1..9, Ctrl+Shift+P). Route all shortcut
      handling through this single map so bindings live in one place.
    - **Settings UI:** a searchable table of Command | Shortcut | Reset, with a
      "press keys to rebind" capture control, conflict detection (warn when two
      commands share a chord), and a *Restore defaults* button.
    - **Persistence:** store the keymap in `SessionState`
      ([`session.rs`](../src/session.rs)) as a serialized list, defaulting to the
      built-in bindings; validate on load and fall back to defaults for anything
      unrecognized (mirror the dock-layout recovery pattern).
    - **Command palette tie-in:** show each command's live binding in the palette
      so the two stay in sync, and reuse the same `Command` enum
      ([`commands.rs`](../src/commands.rs)) as the keymap's key.
    - Ship a small doc/export so users can share a keymap; keep it accessible
      (the shortcut capture must be operable without a mouse).

## Tier 3 — The ML differentiator (2–4 weeks)

11. **Classification.** Logistic regression and a small **MLP** in Burn, with
    softmax / cross-entropy, accuracy / precision / recall / F1, and a
    confusion-matrix view (the heatmap plot family already exists). Roughly
    doubles the IDE's ML reach.
12. **Dataset preparation UI.** Train/val/test split, categorical encoding,
    missing-value strategy, and scaling presets surfaced before training instead
    of being code-only.
13. **Hyperparameter sweeps.** Grid / random search over epochs, learning rate,
    and layer sizes, feeding the existing Runs leaderboard and comparison
    reports.
14. **ONNX import for in-IDE inference.** Today Forge exports ONNX and generates
    services; add loading an arbitrary ONNX model to predict inside the IDE.
15. **Inference playground polish.** Build on the existing single/batch predict
    with an interactive form and live metrics.

## Tier 4 — Engineering health & polish

16. **Split `main.rs`** into `ui/` submodules (menus, panes, editor, dock,
    behavior). At ~9.8k lines it is the main friction point for further work.
17. **Real screenshots** on the homepage (the SVG mockup is a good placeholder,
    but a captured screenshot converts better) and a `CHANGELOG.md`.

---

## Suggested sequencing

1. **Tier 1 as one sweep** — rustfmt, clippy-in-Problems, PR CI, a cargo-tasks
   pane, and the site refresh. A few days, all low-risk, and it makes the IDE
   feel markedly more complete and safer to keep extending.
2. **Classification (#11)** as the flagship ML follow-up.
3. **Editor depth (Tier 2)** interleaved as time allows — the customizable
   shortcuts (#10) pair naturally with the command-palette upgrade (#8).
4. **`main.rs` split (#16)** before or alongside the larger ML work, to keep the
   file from becoming a bottleneck.

## Effort at a glance

| Tier | Scope | Rough effort |
| --- | --- | --- |
| 1 | rustfmt, clippy, CI, cargo tasks, site refresh | 3–5 days |
| 2 | LSP rename/refs/actions/signature, snippets, palette, welcome, custom keybindings | 1–2 weeks |
| 3 | Classification, dataset prep, sweeps, ONNX import | 2–4 weeks |
| 4 | main.rs split, screenshots, changelog | 2–4 days |
