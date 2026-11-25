# Ferro framework guide

## What Ferro is

Ferro is a development platform for native applications. A team creates an application project,
uses Ferro's Rust packages and CLI during development, and selects one or more deployment targets.
Ferro then coordinates the shared application layer with each target's native runtime and build
system.

The intended experience is similar in category to Expo or React Native: one framework helps a team
reach multiple platforms. Ferro differs in implementation and scope: it is Rust-first, preserves
native platform integration, and does not currently promise one universal widget API.

## What Ferro is not

- It is not a single desktop workspace application for end users.
- It is not an AppKit product with Windows and Linux ports.
- It is not a webview shell.
- It is not yet a released SDK or package manager.

The repository's current AppKit host and LiliaDB code are implementation experiments that validate
pieces of the future framework. They are not the developer-facing product contract.

## Intended developer workflow

The public workflow will be built incrementally:

1. Create or adopt a Ferro application project.
2. Define the application in shared Rust packages and declare the targets it supports.
3. Use the `ferro` CLI to diagnose a machine, run a target, build native artifacts, and inspect
   target-specific requirements.
4. Add target-specific code only where a platform capability, lifecycle, UI, or distribution flow
   requires it.
5. Package and release each target using its native signing and distribution conventions.

Today, `ferro init`, `ferro doctor`, `ferro run`, `ferro dev`, `ferro build macos`, and `ferro
target list` establish the macOS workflow. `ferro init` writes a strict, versioned `ferro.toml`
manifest and defaults to a Cargo workspace containing a shared core and executable macOS host. On
macOS, `ferro run --project <path>` builds a Debug `.app` and launches it; `ferro dev --project
<path>` rebuilds and relaunches after successful source changes; and `ferro build macos --project
<path>` produces a local bundle without launching it. The generated project demonstrates the
intended split: its core owns a typed state, events and a pure reducer; its macOS host owns AppKit
views, native commands and file-panel effects.

Bundles are unsigned by default for local development. An optional `[macos] signing_identity` (or
`--sign`) selects an identity already installed in the macOS keychain. `ferro doctor` checks Rust,
Cargo, Xcode, code-signing tools, the generated host and any configured identity. Notarization,
installable distribution and publishing remain out of scope.

## Stable developer API

The first public API is intentionally narrow. `ferro-sdk` owns the application structure, reducer,
lifecycle vocabulary and `FerroError` categories. `ferro-contracts` owns portable Preferences and
Recent Resources services. Native target crates implement those services and own all UI details.

macOS exposes alerts, confirmations, open-file, open-folder and save panels through its runtime.
Their outcomes are returned to the application as typed values or events; cancellation is explicit.
No shared widget API is provided.

Persistence is opt-in and uses LiliaDB exclusively. Applications that do not enable the persistence
feature do not link a storage backend. SQLite is not a Ferro feature or supported alternative.

## Project contract

Every Ferro project has a root `ferro.toml` document. Schema version 1 contains an application
name, a stable reverse-DNS identifier, and an ordered list of desktop targets. Its optional
`[macos] signing_identity` configures local bundle signing. Unknown fields and unknown targets are
rejected so every Ferro tool interprets the project consistently.

`ferro init` defaults to macOS and derives an identifier of the form
`dev.ferro.<slug>`. Use `--id` before distribution to provide an application-owned identifier.

## Portability policy

Ferro shares behaviour before presentation. Product logic, state, types, testing utilities, and
portable capability contracts belong in shared packages. Rendering and integration belong to a
target adapter when their platform semantics differ.

This keeps a Ferro application from being reduced to the least capable target while avoiding a
separate product implementation for every platform. It also lets the framework add iOS, Android,
and later device families without treating desktop assumptions as universal.

The first native capability is intentionally macOS-specific: a host can present open-file,
open-folder and save panels, then turn their selected paths or cancellation into portable events.
Ferro does not claim that this is already a shared capability contract; that requires validation by
at least one additional target runtime.

## Delivery sequence

1. **Desktop foundation** — define the manifest, CLI contract, SDK package boundaries, and the
   supported macOS development workflow.
2. **Supported desktop targets** — prove native runtimes, builds, diagnostics, packaging, and
   integration testing on Windows and Linux as well as macOS.
3. **Mobile foundation** — define the target lifecycle and native bridge model, then validate iOS
   and Android independently.
4. **Additional surfaces** — add watch, TV, automotive, or other targets only with a concrete
   runtime and distribution plan.

Each stage must graduate with a real application built through Ferro, not merely a cross-compiled
crate or a placeholder host.
