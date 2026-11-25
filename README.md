# Ferro

**A native-first Rust desktop architecture for macOS, Windows, and Linux.**

Ferro shares business logic, state, contracts, data models, and tests across desktop targets.
It deliberately does **not** share a lowest-common-denominator widget tree: each platform owns
its native UI, lifecycle, accessibility, windowing, and packaging.

## Philosophy

- **Rust owns the product.** Domain rules, use cases, persistence contracts, and application state
  live in shared Rust crates.
- **The operating system owns the interface.** AppKit, WinUI, and GTK render the application using
  their own controls and conventions.
- **Platform boundaries are explicit.** A platform adapter implements a shared contract; no UI crate
  may depend on another UI crate.
- **The terminal is first-class.** The `ferro` CLI will provide local diagnostics, builds, packaging,
  and release checks without hiding the underlying native SDKs.

## Workspace layout

```text
apps/
  ferro-macos/       Native AppKit host
  ferro-windows/     Reserved Windows native host
  ferro-linux/       Reserved GTK4 native host
crates/
  ferro-domain/      Business types and invariants
  ferro-application/ Platform-neutral use cases
  ferro-contracts/   Ports implemented by system adapters
  ferro-state/       Presentation state and user intents
  ferro-storage/     Persistent LiliaDB or SQLite adapters
  ferro-design/      Semantic design tokens, never widgets
  ferro-testkit/     In-memory adapters and test helpers
tools/
  ferro-cli/         Developer CLI
  xtask/             Repository automation
docs/
  architecture.md    Boundaries, dependency rules, and platform policy
```

See [the architecture guide](docs/architecture.md) for the dependency graph and the platform
implementation strategy.

## Status

Ferro is in bootstrap. The Rust workspace, quality gates, CI matrix, CLI foundation, and native
AppKit workspace-list screen exist. Recent workspaces persist in LiliaDB by default; SQLite is an
explicit alternate feature for deployments that require direct SQLite interoperability.

## Prerequisites

- Rust 1.100 or newer
- macOS development: a current Xcode installation and command-line tools
- Windows and Linux development will require their respective native SDKs when their hosts land

## Development

```sh
# Run all unit and documentation tests.
cargo test --workspace

# Enforce formatting and lints.
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings

# Inspect the current machine.
cargo run -p ferro -- doctor

# Launch the macOS AppKit host.
cargo run -p ferro-macos

# Launch the same host with SQLite instead of the default LiliaDB backend.
cargo run -p ferro-macos --no-default-features --features sqlite
```

## Non-goals

- A webview-based desktop shell
- A custom GPU rendering engine
- A universal `Button`, `List`, or navigation abstraction masking OS behavior
- Claiming that Xcode, Windows tooling, or Linux packaging can be eliminated

## Roadmap

1. Add native file selection and workspace opening to the macOS host.
3. Validate GTK4/Linux and Windows native hosts through dedicated spikes.
4. Package, test, and release a desktop beta on all three operating systems.

## License

The intended license is MIT OR Apache-2.0. The license texts will be added before the first public
release.
