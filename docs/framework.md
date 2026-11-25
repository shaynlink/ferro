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
- It is not yet a released SDK, project generator, or package manager.

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

Today, only the diagnostic seed of this workflow exists. The project manifest, app templates,
target selection, build orchestration, and publishing commands remain to be designed and shipped.

## Portability policy

Ferro shares behaviour before presentation. Product logic, state, types, testing utilities, and
portable capability contracts belong in shared packages. Rendering and integration belong to a
target adapter when their platform semantics differ.

This keeps a Ferro application from being reduced to the least capable target while avoiding a
separate product implementation for every platform. It also lets the framework add iOS, Android,
and later device families without treating desktop assumptions as universal.

## Delivery sequence

1. **Desktop foundation** — define the manifest, CLI contract, SDK package boundaries, and
   supported macOS/Windows/Linux development workflow.
2. **Supported desktop targets** — prove native runtimes, builds, diagnostics, packaging, and
   integration testing on every desktop OS.
3. **Mobile foundation** — define the target lifecycle and native bridge model, then validate iOS
   and Android independently.
4. **Additional surfaces** — add watch, TV, automotive, or other targets only with a concrete
   runtime and distribution plan.

Each stage must graduate with a real application built through Ferro, not merely a cross-compiled
crate or a placeholder host.
