# Ferro

**A Rust-first framework and toolchain for building native applications across platforms.**

Ferro is a developer platform: application teams use a CLI, shared Rust packages, and target
adapters to build and ship their own products. In that sense it is comparable to Expo or React
Native. Ferro is not itself the end-user desktop application described by its early prototype code.

Desktop is the first delivery surface (macOS, Windows, and Linux). iOS and Android follow once the
desktop foundation is stable; watchOS, tvOS, automotive, and other surfaces are later targets.

## Product direction

Ferro provides a common foundation for an application's product logic, development workflow, and
platform integration:

- **CLI and tooling** for creating, diagnosing, building, running, packaging, and eventually
  publishing application projects.
- **Shared Rust SDK packages** for domain logic, state, contracts, testing, and platform-neutral
  capabilities.
- **Target adapters** that connect an application to each platform's native lifecycle, UI,
  permissions, packaging, and distribution rules.
- **Native output**, not a browser shell or a lowest-common-denominator desktop app.

Ferro takes inspiration from Expo and React Native as a developer experience, not as a commitment
to JavaScript, a webview, or a universal widget hierarchy. The framework will share what is
meaningfully portable and preserve each target's native requirements.

Read [the framework guide](docs/framework.md) for the intended developer model and
[the architecture guide](docs/architecture.md) for package and target boundaries.

## Repository status

This repository is an early bootstrap, not a general-purpose SDK release. It currently contains:

- a Rust workspace and cross-OS quality-gate matrix;
- a `ferro` developer CLI for macOS project generation, diagnostics, development and local bundles;
- shared crates that prove the intended package boundaries;
- macOS, Windows, and Linux host placeholders, with a small AppKit reference host on macOS;
- an internal LiliaDB-backed storage experiment.

The AppKit screen and workspace catalogue are reference/prototyping code. They do not define
Ferro's product model or public API.

## Current workspace

```text
apps/
  ferro-macos/       macOS reference host
  ferro-windows/     Windows target placeholder
  ferro-linux/       Linux target placeholder
crates/
  ferro-domain/      shared types and invariants prototype
  ferro-application/ shared use-case prototype
  ferro-contracts/   target-service boundaries
  ferro-state/       shared state primitives
  ferro-design/      semantic design intent, never a widget kit
  ferro-storage/     internal persistence experiment
  ferro-testkit/     reusable test doubles and fixtures
tools/
  ferro-cli/         developer-facing command foundation
  xtask/             repository automation
docs/
  framework.md       developer-platform model and roadmap
  architecture.md    package and target boundaries
  storage.md         status of the internal storage experiment
```

## Development

Ferro currently requires Rust 1.100 or newer. Developing the reference macOS host also requires
Xcode and its command-line tools.

```sh
# Run the shared checks.
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Inspect the local development environment.
cargo run -p ferro -- doctor

# Create, diagnose and run a macOS development project.
cargo run -p ferro -- init /tmp/atlas --targets macos
cargo run -p ferro -- doctor --project /tmp/atlas
cargo run -p ferro -- run --project /tmp/atlas

# Inspect every target known by this Ferro version.
cargo run -p ferro -- target list

# Launch the repository's AppKit reference host (not a Ferro application template).
cargo run -p ferro-macos
```

`ferro init` creates a Cargo workspace and a strict `ferro.toml` manifest. It defaults to macOS,
the sole executable target today; Windows and Linux remain explicitly declarable only. A generated
macOS project contains a portable typed reducer and a native AppKit host with lifecycle callbacks,
a persistent controller, a File menu (`Cmd+O`) and a native file picker.

On macOS, `ferro run --project <path>` compiles the project, assembles a Debug `.app` under
`target/ferro/macos/debug`, then launches it. `ferro dev --project <path>` keeps that application
running, watches Rust and Cargo inputs, and relaunches after a successful rebuild. `ferro build
macos --project <path>` produces the same Debug bundle without launching; use `--release` for an
optimized bundle and `--output <directory>` to choose its destination. The generated project links
to the checked-out Ferro runtime; this is a source-checkout developer preview, not yet a published
SDK dependency.

The optional `[macos] signing_identity` in `ferro.toml` (or `ferro build macos --sign <identity>`)
signs a local bundle with an identity present in the macOS keychain. Without it, bundles stay
unsigned for local development. `ferro doctor` verifies Rust, Cargo, Xcode, `codesign`, the project
structure and any configured identity. Notarization, installable distribution and publication are
still roadmap work.

The public developer surface is deliberately small: `ferro-sdk` defines an application reducer and
typed `FerroError`; target hosts own native UI and dialogues. Optional persistence exposes only
preferences and recent resources, backed exclusively by LiliaDB. Enable it in a generated macOS
host with `cargo run -p <app>-macos --features persistence`; no SQLite backend is supported.

## Roadmap

1. Graduate Windows and Linux to supported native development targets.
2. Establish a mobile target abstraction and validate iOS and Android with focused native spikes.
3. Add notarized, distributable signing and release workflows per target.
4. Extend to additional device surfaces only after the core project and target model is stable.

## License

The intended license is MIT OR Apache-2.0. License texts will be added before the first public
release.
