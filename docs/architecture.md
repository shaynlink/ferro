# Ferro architecture

## Purpose

Ferro is a Rust monorepo for desktop applications that need substantial code sharing without
pretending that macOS, Windows, and Linux have the same UI model. The architecture shares product
behavior and preserves native user experience.

## Dependency graph

```text
                 ferro-domain
                       |
             ferro-application
                       |
                ferro-contracts
                       |
        +--------------+--------------+
        |              |              |
apps/ferro-macos apps/ferro-windows apps/ferro-linux
        |              |              |
      AppKit         WinUI         GTK4
```

`ferro-state` and `ferro-design` are shared support crates. They may depend on domain types, but
they must not depend on a platform UI crate.

## Layer responsibilities

### Domain

`ferro-domain` contains durable business names, values, rules, and invariants. It has no I/O,
runtime, UI, OS, or database dependency.

### Application

`ferro-application` implements use cases. It receives abstractions from `ferro-contracts`, maps
user intents to domain actions, and returns data that a platform host can present.

### Contracts

`ferro-contracts` defines ports for system capabilities such as persistence, native file dialogs,
secure storage, notifications, and background scheduling. Contracts contain no platform type.

### State and design intent

`ferro-state` represents user intents and screen state. `ferro-design` holds semantic intent such
as density and spacing. Neither crate exports a cross-platform widget abstraction.

### Platform hosts

Each host has full ownership of its UI implementation:

| Target | Native direction | Host responsibility |
|---|---|---|
| macOS | AppKit through `objc2` | Windows, menus, commands, accessibility, sandbox and distribution |
| Windows | WinUI validation spike | Fluent controls, packaging, notification and system integration |
| Linux | GTK4 validation spike | GNOME/Wayland integration, desktop entries and Flatpak packaging |

The macOS host isolates its Objective-C FFI boundary in one crate. `unsafe` remains forbidden in
the shared crates; a host may allow a documented, narrow FFI call when a native binding requires
it.

## Rules

1. A platform UI crate may depend on shared crates; it may never depend on another platform UI
   crate.
2. Shared crates must compile without Apple, Windows, GTK, or GUI dependencies.
3. A platform detail becomes a contract only when at least two platform hosts need the same
   **semantic capability**. Similar-looking widgets are not enough reason to generalize.
4. UI callbacks emit `ferro-state` intents or invoke application use cases. They do not embed
   business rules.
5. Every platform adapter needs contract tests. A test double belongs in `ferro-testkit` when it is
   useful beyond one platform.
6. Any native FFI `unsafe` block must be local, minimal, and include a safety justification.

## Build and validation policy

Development begins on macOS because it is the local host. That does not make a macOS build proof
of Windows or Linux behavior.

CI runs formatting, linting, and tests in a macOS, Windows, and Linux matrix. Each platform host
will add its own smoke test and packaging job when implemented. A release requires native runtime
validation on the target OS, not only cross-compilation.

## Near-term milestones

1. Implement a local recent-workspace persistence adapter and test it against the shared contract.
2. Add native file selection and workspace opening to the macOS host.
3. Define the release artifact strategy for `.dmg`, Windows installer/MSIX, and Flatpak.
4. Perform isolated GTK4 and WinUI feasibility spikes before adding their production host crates.
