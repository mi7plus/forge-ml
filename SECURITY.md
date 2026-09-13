# Security policy

## Reporting a vulnerability

Please report security issues **privately**, not in a public issue.

- Preferred: open a private report via GitHub's **[Report a vulnerability](https://github.com/mi7plus/forge-ml/security/advisories/new)**
  (Security → Advisories) on this repository. This keeps the details private
  until a fix is available.

Include what you'd need to reproduce it: affected version, platform, and steps
or a proof of concept. We aim to acknowledge a report within a few days and will
coordinate a fix and disclosure timeline with you.

Please do not run denial-of-service, data-destruction, or mass-scanning tests
against any hosted resource; a local proof of concept is enough.

## Supported versions

Fixes land in the latest release. Always update to the newest installer from
[Releases](https://github.com/mi7plus/forge-ml/releases/latest) before reporting,
in case the issue is already fixed.

## Scope

In scope: the `forge_ide` app and its helpers (`forge`, `forge_manager`,
`forge_webview`, `forge_cef`), the environment/provisioning system, and the
release/update pipeline. Because the app runs user code and shells out to trusted
developer tools (Cargo, Git, Python, database and cloud clients), reports should
concern Forge's own handling — credential storage, update verification, the
provisioning download-and-verify path, subprocess/argument construction, or the
bundled web engine — rather than the behavior of a tool the user explicitly ran.

## Bundled dependencies with their own release cadence

- **Chromium via CEF (the in-app web preview).** The Windows installer bundles a
  pinned Chromium build (the `cef` crate, currently the 152 line). A browser
  engine accumulates security fixes over time, and Forge's copy updates **only
  when Forge is released with a newer `cef` crate** — there is no independent
  auto-update for it. We track upstream CEF/Chromium and bump the pin on
  releases; users should keep Forge updated. The preview renders local files and
  pages the user opens; it runs out of process (`forge_cef`) so a page crash
  can't take the IDE down, but it is still a full web engine and should be
  treated as one.
- **The wider dependency tree** (burn, wgpu, arrow, polars, TLS stacks, …) is
  scanned weekly by the "Security audit" workflow (`cargo audit`). Advisories we
  cannot act on today — transitive pins, or unmaintained-only crates — are
  acknowledged with their blocker in [`.cargo/audit.toml`](.cargo/audit.toml), so
  the job fails only on new, actionable findings.

## Update integrity

Release artifacts and their update-channel manifests carry GitHub
build-provenance attestations. Forge can discover and report an available update
and verify the artifact's SHA-256, but **installing an update is always an
explicit user action** — Forge never replaces its own executable silently.
OS code signing / notarization is not yet in place, so the first launch shows a
publisher warning; verify a download against its `SHA256SUMS` before bypassing it.
