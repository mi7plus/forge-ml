# Integration and extension guide

Forge ML currently supports integrations through stable process and event boundaries rather than dynamically loaded native plugins. This keeps third-party code outside the GUI process.

An integration can:

1. Emit versioned Forge events or supported compatibility records on stdout.
2. Write artifacts under `.forge/artifacts` and reference safe relative paths.
3. Generate editable Rust notebook cells or standalone Cargo projects.
4. Implement a CLI adapter that returns rectangular data normalized into Arrow.

Millwright and Burn are core native dependencies and ship in every Forge build. Millwright integration must use its published crates.io package; do not depend on a neighboring checkout. Burn uses the published package with training, metrics, Flex CPU, and WGPU support. Keep unrelated platform-specific native dependencies behind explicit boundaries. Python support stops at runtime/kernel execution and package discovery; Python ML packages remain user-managed.

Credential-bearing integrations must delegate to an OS credential manager or the external tool's credential chain. Persist only non-secret profile metadata. Bound network/process operations, limit previews, validate URLs and relative paths, and redact command errors.

A future plugin ABI should be built on `forge-protocol`, not `ForgeApp` internals.
