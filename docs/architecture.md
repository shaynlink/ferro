# Ferro architecture

## Purpose

Ferro is a framework and developer toolchain for teams building native applications. It is not the
application being built. A Ferro project will eventually use the framework's CLI, SDK packages,
and target adapters to produce applications for desktop first, then mobile and other device
surfaces.

Ferro is comparable to Expo or React Native in its position in a developer's workflow: it reduces
the cost of targeting multiple platforms while keeping a platform's real capabilities available.
Its implementation is Rust-first and it does not assume a JavaScript runtime, a webview, or a
single cross-platform widget tree.

## Target model

```text
application project
        |
   ferro CLI and project tooling
        |
 shared Ferro SDK packages
        |
 target adapter and native runtime
        |
macOS | Windows | Linux | iOS | Android | future surfaces
```

The shared layer owns portable concepts: application logic, data types, state, capability
contracts, testing support, and developer tooling. A target adapter owns its platform's lifecycle,
rendering, input, accessibility, permissions, signing, packaging, and store/distribution rules.

The initial target family is desktop:

| Target | Direction | Current repository state |
|---|---|---|
| macOS | Native macOS runtime, currently AppKit-based | Reference host exists |
| Windows | Native Windows runtime to be validated | Placeholder only |
| Linux | Native Linux runtime to be validated | Placeholder only |
| iOS / Android | Mobile runtime and lifecycle adapters | Future target family |
| watchOS / tvOS / automotive | Specialised device adapters | Long-term exploration |

## Repository boundaries

The current Cargo workspace is the framework's bootstrap. Its names and APIs are not yet a stable
public SDK, but its dependency direction is deliberate:

```text
ferro-domain
      |
ferro-application ---- ferro-state / ferro-design
      |
ferro-contracts
      |
target hosts and adapters
```

- `ferro-domain` and `ferro-application` are early shared-core prototypes. They must remain free
  of UI, OS, and target SDK dependencies.
- `ferro-contracts` expresses portable capabilities. A capability enters this layer only when it
  has a framework-level meaning, not merely because two targets happen to have similarly named UI
  controls.
- `ferro-state`, `ferro-design`, and `ferro-testkit` support shared application development. The
  design crate contains semantic intent, never a fake universal widget kit.
- `apps/ferro-*` are target-host experiments and reference runtimes, not Ferro's end-user
  application. They may depend on shared crates; they never depend on one another.
- `tools/ferro-cli` is the seed of the developer experience. It will grow around an explicit
  project model rather than absorbing product-specific application behaviour.

## Architectural rules

1. Ferro exposes a framework contract, not a bundled product UI or workspace manager.
2. Shared code cannot import Apple, Windows, GTK, Android, or other target SDKs.
3. Target adapters may use native APIs directly and are responsible for target-specific UX,
   accessibility, permissions, and distribution requirements.
4. Cross-platform abstractions represent capabilities and lifecycle semantics, never cosmetic
   equivalence between native controls.
5. A target-specific experiment must prove its native runtime before it becomes a supported Ferro
   target.
6. Platform FFI remains narrow, documented, and isolated in the relevant target adapter.
7. Public SDK and CLI contracts require compatibility policy, examples, and contract tests before
   they are declared stable.

## Development and validation

Development currently starts on macOS because that is the available machine, but macOS success is
not proof of a framework target. CI runs Rust quality checks on macOS, Windows, and Linux; each
future target needs native runtime, packaging, and accessibility validation on its own platform.

The next architectural milestone is not another reference-app screen. It is an explicit Ferro
project model and CLI/SDK contract that application teams can use to target desktop platforms.
