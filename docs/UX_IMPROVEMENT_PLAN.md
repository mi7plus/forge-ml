# Forge ML — UI/UX & Docking Improvement Plan

Status: in progress (2026-08-30). Scope: interaction and layout improvements to
the `egui`/`eframe` desktop shell, plus supporting examples, tests, and a public
presentation site. This plan does **not** change ML/runtime semantics.

**Shipped so far:** Phase 1.1 (menu hover-to-open), Phase 1.2 (dockable panes via
`egui_tiles`), Phase 1.3 (View-menu show/hide + reset layout), and Phase 1.4
(dock layout persisted across restarts). Phases 2–3 remain open.

## Context (what the code does today)

- The shell is built with `egui`/`eframe` 0.36 and lives almost entirely in
  [`src/main.rs`](../src/main.rs) (~9k lines). Layout uses egui's unified
  `Panel` API — a top menu bar, a top command bar, a left workspace, a right
  inspector, a bottom console, and a `CentralPanel` editor
  ([`src/main.rs:7316`](../src/main.rs)).
- The **menu bar** ([`src/main.rs:2403`](../src/main.rs)) draws top-level menus
  with bare `ui.menu_button("File", …)` calls inside a `ui.horizontal`. Because
  they are **not** wrapped in an `egui::MenuBar`, egui does not coordinate the
  "hover to switch while one menu is open" behavior — hence the reported bug.
- The **right inspector** is a *single* pane that swaps between **15**
  `InspectorTab` variants rendered as `selectable_label`s in a
  `horizontal_wrapped` row ([`src/main.rs:3180`](../src/main.rs)): Variables,
  Data, Plots, Runs, Search, Help, Problems, Git, Crates, GitHub, Studio, SQL,
  Deep, Deploy, Storage. Only one is visible at a time; there is no per-tab
  hide/show and no way to move a tab out of the right edge.
- Docking already exists — but only for the **dataset viewer**, which toggles
  between a floating `egui::Window` and a docked bottom-right split via
  `dataset_viewer_docked` ([`src/main.rs:6167`](../src/main.rs),
  [`src/pane_layout.rs`](../src/pane_layout.rs)). This is the pattern to
  generalize.
- The **View menu** ([`src/main.rs:2556`](../src/main.rs)) currently holds only a
  theme toggle and a static hint label.
- A **command palette** (Ctrl+Shift+P), F6 pane cycling, and Ctrl+1 already
  exist (see [README](../README.md) and [user guide](USER_GUIDE.md)); the plan
  extends these rather than reinventing them.
- Layout/appearance state persists through `SessionState`
  ([`src/session.rs:85`](../src/session.rs)) and `WorkspaceRecovery`
  ([`crates/forge-storage/src/lib.rs:9`](../crates/forge-storage/src/lib.rs)).

## Decision: adopt `egui_tiles` for dockable zones

The user asked for **dockable zones** (drag tabs between left/right/bottom
regions, split, and re-dock), not just free-floating windows. Two viable paths:

| Option | Pros | Cons |
| --- | --- | --- |
| **Custom dock manager** | No new dep; full control | Re-implements drag/drop, split trees, drop-zone hit-testing — weeks of fiddly work and its own bugs |
| **`egui_tiles` 0.17** (emilk) | Purpose-built tiling + drag/drop + splits; same author as egui | New dependency; requires migrating the fixed-`Panel` layout into a tile tree |

**Recommendation: `egui_tiles` 0.17.1.** Verified against this repo:
`cargo add egui_tiles --dry-run` resolves to **0.17.1** and **keeps `egui`
pinned at `=0.36.0`** (confirmed with `cargo tree` — no version bump, no other
crate disturbed). It gives us drag-between-zones, splitters, and tab bars for
free, and it serializes its tree via `serde`, which slots directly into the
existing session-persistence layer.

Migration is incremental: the editor stays in the `CentralPanel`; the
left/right/bottom **tool panes** become `egui_tiles` tiles. Each current
`InspectorTab` render function (`self.git_inspector(ui)`,
`self.data_inspector(ui)`, …) becomes a tile `Pane`, so the bodies are reused
verbatim.

---

## Phase 1 — The three explicit asks

### 1.1 Menu bar: hover-to-open ✅ quick win

Wrap the top-level menus in `egui::MenuBar::new().ui(ui, |ui| { … })` at
[`src/main.rs:2404`](../src/main.rs) instead of the bare `ui.horizontal`. egui's
`MenuBar` tracks a shared "a menu is open" state and switches menus on hover
automatically — no per-menu bookkeeping. While there, add the missing
`ui.close()` calls (e.g. `Open project`, `Save`, `Run cell`, the theme toggle)
so items don't leave the menu hanging open, and give each top-level menu a
stable `Id` so hover routing is deterministic.

Effort: ~1–2 hrs. Risk: low. Ships independently of the rest.

### 1.2 Dockable, hideable tool panes

Introduce a `PaneKind` enum (superset of today's `InspectorTab` + the left
`Project`/`Outline` + bottom `Console`/`History`/`Python`) and a `PaneRegistry`:

```rust
struct PaneState {
    kind: PaneKind,
    visible: bool,          // shown at all (View menu toggle)
}
// Layout/geometry is owned by egui_tiles' Tree<PaneKind>.
```

- Build one `egui_tiles::Tree<PaneKind>` with a default layout matching the
  current look: left column (Project/Outline), right column (the inspector
  tabs), bottom row (Console/History/Python), editor in the center container.
- Implement `egui_tiles::Behavior for ForgeApp`:
  - `pane_ui` dispatches to the existing render fns (reuse
    `git_inspector`, `data_inspector`, `charts`, … from
    [`src/main.rs:3209`](../src/main.rs)).
  - `tab_title_for_pane` returns the current labels/icons.
  - Enable `Behavior::simplification_options` so emptied containers collapse.
- Dragging a tab tab-bar → drop zone re-docks it (left/right/bottom) or splits —
  provided by `egui_tiles` out of the box.
- Right-click a tab → context menu: **Hide**, **Move to left/right/bottom**,
  **Float** (detach into an `egui::Window` tile).

Retire the `dataset_viewer_docked` special case by making the dataset viewer
just another `PaneKind::DataViewer` tile; the float/dock buttons become the
generic tab context-menu actions.

Effort: 2–4 days. Risk: medium (central layout refactor) — mitigate by landing
1.1 first and keeping a feature flag / "Reset layout" escape hatch.

### 1.3 View menu → pane visibility + layout controls

Replace the near-empty View menu ([`src/main.rs:2556`](../src/main.rs)) with:

- A checkbox per pane (`ui.checkbox(&mut pane.visible, label)`) grouped by region
  (Tools / Editor / Console). Toggling hides/shows the tile.
- **Reset layout to default** — rebuild the default `Tree`.
- **Save current layout as default** — persist the current tree.
- Keep the existing theme toggle; add **high contrast** and **reduced motion**
  toggles here too (they exist in Settings; surfacing them in View is cheap).

### 1.4 Persistence

Extend `SessionState` ([`src/session.rs:85`](../src/session.rs)) and
`WorkspaceRecovery` with a serialized `egui_tiles::Tree<PaneKind>` and the
per-pane `visible` flags. `egui_tiles::Tree` is `Serialize`/`Deserialize`, so
this is additive. Guard deserialization: on a schema mismatch, fall back to the
default layout rather than failing the restore (mirror the existing
`dataset_viewer_docked` recovery guard at [`src/main.rs:990`](../src/main.rs)).

---

## Phase 2 — UI/UX polish (independent, low risk)

- **Tab icons + overflow.** With 15+ inspector tabs the wrapped label row is
  cramped. Use the already-present `egui_phosphor_icons` for compact
  icon+tooltip tabs and a "More ▾" overflow for the long tail. (The toolbar
  already uses this dependency at [`src/main.rs:2579`](../src/main.rs).)
- **Status bar** across the bottom edge: run/kernel state, active file + cursor
  position, git branch, LSP status. These signals are already computed and
  scattered; a single bottom strip consolidates them.
- **Command palette reach.** The palette already exists; register every menu
  action and every `PaneKind` (show/focus) as palette commands so the new panes
  are keyboard-reachable. Add Ctrl+1..9 to focus panes by index.
- **Per-pane state memory:** remember scroll position and last sub-selection per
  tool pane across focus changes.
- **Drag-to-reorder editor tabs** and a middle-click-to-close affordance in the
  editor tab strip.

## Phase 3 — Functional / ML-specific improvements

- **Debug menu is a dead stub** ([`src/main.rs:2505`](../src/main.rs)). Either
  wire a minimal run/inspect flow or hide it behind Phase 1's visibility toggles
  so it isn't a visible dead end.
- **Floating window geometry** persistence for detached panes (position/size).
- **Consistent dock model** for the left (`Project`/`Outline`) and bottom
  (`Console`/`History`/`Python`) tabs — they become `PaneKind`s in the same
  tree, so this falls out of Phase 1 for free.

---

## Phase 4 — Examples (requested)

Add runnable, convention-matching examples under
[`examples/notebooks/`](../examples/notebooks) (Forge `//# %%` cells; see
[`examples/README.md`](../examples/README.md)):

- **`native_regression.rs`** — walks the embedded Burn native-regression path:
  build a small dataset, emit a `forge_metric:loss` curve and a `forge_table`,
  mirroring the Deep Learning pane workflow described in the
  [user guide](USER_GUIDE.md).
- **`structured_plots.rs`** — emits versioned `forge_plot:` JSON exercising
  several of the 11 plot families (line, scatter, histogram, bar) so the Plots
  pane and its exports have a one-file smoke test.
- **`dockable_layout_tour.rs`** — a guided notebook whose cell comments walk a
  new user through opening panes, docking, and hiding them (doubles as manual QA
  for Phase 1).

Update [`examples/README.md`](../examples/README.md) to index the new files.
(The first two are added in this change; the tour lands with Phase 1.)

## Phase 5 — Tests (requested)

- **Now (pure logic, no UI):** extend
  [`src/pane_layout.rs`](../src/pane_layout.rs) tests — exact boundary at
  `MIN_RIGHT_PANE_HEIGHT * 2`, idempotence of `resolve`, and drag past both
  clamps. (Added in this change.)
- **With Phase 1:** unit-test the `PaneRegistry` (visibility toggles, default
  layout construction) and round-trip the serialized `Tree` through
  `SessionState` to prove layout survives restart — the pattern already used by
  the `dataset_viewer_docked` tests in
  [`src/session.rs`](../src/session.rs).
- Keep everything on the existing `cargo test` path so CI covers it.

## Phase 6 — Presentation site (requested)

A static **GitHub Pages** site under [`site/`](../site) with a **homepage**
(`index.html`) and a **guide** (`guide.html`), deployed by
[`.github/workflows/pages.yml`](../.github/workflows/pages.yml) using
`actions/deploy-pages`. Content is sourced from the [README](../README.md) and
[user guide](USER_GUIDE.md) so it stays accurate. (Added in this change; enable
Pages → "GitHub Actions" in repo settings to publish.)

---

## Sequencing & risk

1. **1.1 menu hover fix** — land first, standalone, instantly verifiable.
2. **Phase 6 site + Phase 4 examples + Phase 5 tests** — independent, no risk to
   the shell; can land in parallel.
3. **1.2 → 1.3 → 1.4 docking** — the one larger refactor; behind a
   "Reset layout" escape hatch and default-layout fallback on restore.
4. **Phases 2–3** — incremental polish afterward.

Effort estimate: Phase 1 ≈ 3–5 focused days; Phases 4–6 ≈ 1 day (mostly landed
here); Phases 2–3 ≈ 2–3 days.
