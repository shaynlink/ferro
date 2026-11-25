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
- an embryonic `ferro` developer CLI (`init`, `doctor`, target discovery, and storage diagnostics);
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

# Create a portable desktop project scaffold.
cargo run -p ferro -- init /tmp/atlas
cargo run -p ferro -- doctor --project /tmp/atlas
cargo run -p ferro -- target list

# Launch the macOS reference host (not a Ferro application template).
cargo run -p ferro-macos
```

`ferro init` creates a Cargo workspace and a strict `ferro.toml` manifest. Its default targets are
macOS, Windows, and Linux; only the macOS reference host exists today. Development runtime
management, target builds, signing, packaging, and publishing remain roadmap work.

## Roadmap

1. Turn the declared desktop target adapters into supported development and build targets.
2. Establish a mobile target abstraction and validate iOS and Android with focused native spikes.
3. Add repeatable packaging, signing, and release workflows per target.
4. Extend to additional device surfaces only after the core project and target model is stable.

## License

The intended license is MIT OR Apache-2.0. License texts will be added before the first public
release.
