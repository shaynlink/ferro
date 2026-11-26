# Developer, Debugging, Profiling, Testing, and Telemetry Tools

Ferro includes an integrated developer toolchain across `ferro-devtools`, `ferro-testkit`, `ferro-state`, `ferro-macos-runtime`, `ferro` CLI, and `xtask`:

## Hot-Reload Development (`ferro dev`)

`ferro dev --project <path>` launches the native target with `FERRO_DEVTOOLS=1` and `FERRO_DEV_STATE_FILE`:

- **Asset & resource hot-reload**: changes in `assets/` or `macos.resources_dir` are synced straight into `.app/Contents/Resources` and signalled via `POST /dev/reload` (`Lifecycle::Reloaded` / `MacosHost::on_hot_reload`) without recompiling Rust.
- **Debounced Rust hot-restart**: source/Cargo changes debounce (`--debounce-ms`), optionally run `--test-on-save`, rebuild, snapshot/restore state via `MacosHost::save_dev_snapshot` / `restore_dev_snapshot`, and relaunch.
- **Flags**: `--release`, `--features`, `--devtools-port`, `--no-asset-reload`, `--preserve-state`, `--once`.

## Debugging (`ferro debug` & `DebugSession` / `TimeTravelStore`)

- `ferro-state::TimeTravelStore`: retains state transition snapshots with `jump_to`, `step_back`, and `step_forward`.
- `ferro-macos-runtime`: injects a **Debug** menu (`Cmd+Opt+D` to toggle the native `DevTools` HUD window, `Cmd+Opt+R` to trigger hot reload) and serves loopback-only (`127.0.0.1`) diagnostics when `FERRO_DEVTOOLS=1` or `FERRO_DEVTOOLS_PORT` is set.
- `ferro debug launch [--lldb] [--hud] [--devtools-port <port>]`: builds and launches under DevTools or `lldb`.
- `ferro debug attach --pid <pid> [--backtrace]`: attaches `lldb` or captures all-thread backtraces.
- `ferro debug inspect [--bundle <path> | --project <path>]`: inspects `Info.plist`, Mach-O architectures (`lipo`), linked libraries (`otool -L`), bundle footprint, and `codesign` status.
- `ferro debug logs [--app <name>] [--system-log] [--last 10m]`: inspects `.ips`/`.crash` reports and macOS unified logs.
- `ferro debug state [--url <url> | --pid <pid>]`: reads live reducer state and event trace from `/trace`.

## Profiling (`ferro profile` & `ferro_devtools::Profiler`)

- `ferro-devtools::Profiler`: records hierarchical spans (`reduce`, `handle_effect`, `render`) and frame-budget drops, exporting Chrome Trace JSON (`chrome://tracing` / Perfetto), folded stacks, and pure-Rust SVG flamegraphs.
- `ferro profile cpu --pid <pid> [--duration 3] [--flamegraph out.svg] [--folded out.txt]`: samples live macOS CPU call stacks via `/usr/bin/sample` and generates SVG flamegraphs.
- `ferro profile memory --pid <pid> [--leaks]`: runs `vmmap -summary`, `heap -s`, and optional `leaks`.
- `ferro profile build [--project <path>] [--release]`: runs `cargo build --workspace --timings=html`.
- `ferro profile binary [--project <path>] [--top 15]`: reports bundle/executable sizes, Mach-O segments (`size -m`), and largest symbols (`nm`).
- `ferro profile trace [--chrome-trace out.json] [--flamegraph out.svg]`: exports live DevTools spans & flamegraphs.

## Network Inspection (`ferro network` & `NetworkInspector`)

- `ferro-devtools::NetworkInspector`: records HTTP/IPC requests with automatic redaction of sensitive headers (`Authorization`, `Cookie`, `Set-Cookie`, `X-Api-Key`), filtering, latency statistics, and HAR 1.2 export.
- `ferro network connections [--pid <pid>] [--process <name>] [--listening]`: lists active TCP/UDP sockets via `lsof -i -n -P`.
- `ferro network traffic [--duration-ms 800] [--pid <pid>]`: measures per-interface and per-process RX/TX throughput (`netstat -ibn` + `nettop`).
- `ferro network probe <url>`: measures DNS lookup, TCP connect, TLS handshake, TTFB, and total HTTP/TCP latency.
- `ferro network inspect [--host <filter>] [--errors-only] [--har out.har]`: queries live DevTools `/network` and exports HAR 1.2.

## System & Hardware Telemetry (`ferro monitor`)

`ferro monitor [--section all|cpu|memory|gpu|neural-engine|network|storage] [--interval-ms 600] [--count 1] [--pid <pid>] [--project <path>] [--json]`:

- **CPU**: brand, physical/logical/P/E core topology, load averages, live user/system/idle %.
- **Memory**: total/used/available/active/inactive/wired/compressed/purgeable/swap bytes, pageins/pageouts, pressure %.
- **GPU**: Apple GPU model, core count, Metal driver, `Device`/`Renderer`/`Tiler` utilization %, in-use/allocated GPU memory via `IOAccelerator` IORegistry.
- **Neural Engine (ANE)**: architecture, core count, unit count, `IOPowerManagement` state, active `aned`/`aneuserd`/`ANECompilerService` clients, plus optional `powermetrics` milliwatt power when passwordless `sudo` is available.
- **Network Usage**: per-interface and total RX/TX bytes and bytes/s, plus top network processes.
- **SSD / Storage Usage**: mounted filesystem capacities, live block read/write MB/s and IOPS, NVMe SMART health (temperature, wear % used, spare %, lifetime read/written bytes, power-on hours, media errors), and project `target/` / `dist/` / bundle disk footprint.

## UI & Automation Testing + CI (`ferro test`, `ferro ci`, `xtask`)

- `ferro-testkit::TestApp`: deterministic headless reducer/lifecycle driver with time-travel assertions.
- `ferro-testkit::UiTree`, `UiAction`, `AutomationScenario`, `AutomationReport`: UI hierarchy representation, accessibility auditor (`missing-label`, `duplicate-id`, `zero-frame`, `small-hit-target`), and JUnit XML reporting.
- `ferro-macos-runtime::ui_inspector`: walks live `NSWindow` / `NSView` / `NSControl` hierarchies and executes `Click`, `SetText`, `ResizeWindow`, and assertions on the AppKit main thread.
- `ferro test unit [--nextest] [--junit out.xml]`: runs Cargo tests or `cargo-nextest`.
- `ferro test ui [--tree tree.json | --launch] [--scenario s.json] [--strict-a11y] [--screenshot-dir dir] [--junit out.xml]`: runs offline or live macOS UI automation & accessibility audits.
- `ferro ci check [--github-annotations] [--summary-file md] [--json]` & `ferro ci init`: runs manifest, `fmt`, `clippy`, `test`, and bundle checks or generates `.github/workflows/ferro-ci.yml`.
- `cargo run -p xtask -- ci|check|smoke|audit`: repository-wide CI, doc, policy audit, and smoke automation.
