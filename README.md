# Ferro

Ferro is a native-first desktop application architecture for macOS, Windows, and Linux.

The workspace shares Rust domain logic, application services, contracts, data models, and
tests. Each operating system owns its UI, lifecycle, accessibility, windowing, and packaging.

## Workspace rules

- `ferro-domain` contains pure business types and rules.
- `ferro-application` orchestrates use cases through `ferro-contracts`.
- Platform crates implement those contracts and render native views.
- Platform UI crates never depend on one another.
- A shared widget abstraction is deliberately out of scope.

## Current status

The repository currently provides a compile-checked workspace, a small application core, a
terminal-first CLI, and placeholder desktop entry points. The first product milestone is a
native AppKit macOS shell that renders a real workspace list.

## Commands

```sh
cargo test --workspace
cargo run -p ferro -- doctor
cargo run -p ferro-macos
```

