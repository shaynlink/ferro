//! Native `AppKit` `DevTools` HUD window displaying live telemetry, trace, profiler, and UI stats.

use std::fmt::Write as _;
use std::time::Duration;

use ferro_devtools::{DevToolsState, TelemetryCollector};
use ferro_sdk::{WindowDescriptor, WindowId, WindowStyle};
use objc2::MainThreadOnly;
use objc2::rc::Retained;
use objc2_app_kit::{NSAutoresizingMaskOptions, NSFont, NSScrollView, NSTextView, NSWindow};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};

/// Stable window identifier reserved for the runtime's native `DevTools` HUD.
pub const DEVTOOLS_WINDOW_ID: &str = "ferro.devtools";

/// Native `AppKit` `DevTools` window state.
pub struct DevToolsHud {
    text_view: Retained<NSTextView>,
}

impl DevToolsHud {
    /// Returns the window descriptor for the native `DevTools` inspector panel.
    #[must_use]
    pub fn descriptor(app_name: &str) -> WindowDescriptor {
        WindowDescriptor::new(
            WindowId::new(DEVTOOLS_WINDOW_ID),
            format!("{app_name} — Ferro DevTools"),
        )
        .size(860.0, 600.0)
        .min_size(560.0, 380.0)
        .style(WindowStyle::Standard)
    }

    /// Populates the `DevTools` `NSWindow` with a scrollable monospace diagnostics view.
    ///
    /// # Panics
    ///
    /// Panics if `window` has no `contentView`.
    #[must_use]
    pub fn build(window: &NSWindow, main_thread: MainThreadMarker) -> Self {
        let content = window
            .contentView()
            .expect("DevTools runtime windows have a content view");
        let bounds = content.bounds();

        let scroll = NSScrollView::initWithFrame(
            NSScrollView::alloc(main_thread),
            NSRect::new(NSPoint::new(0.0, 0.0), bounds.size),
        );
        scroll.setHasVerticalScroller(true);
        scroll.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );

        let text_view = NSTextView::initWithFrame(
            NSTextView::alloc(main_thread),
            NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(bounds.size.width, bounds.size.height),
            ),
        );
        text_view.setEditable(false);
        text_view.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(12.0, 0.0)));
        text_view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        text_view.setString(&NSString::from_str("Ferro DevTools initializing…"));

        scroll.setDocumentView(Some(&text_view));
        content.addSubview(&scroll);

        Self { text_view }
    }

    /// Refreshes the HUD text using current `DevToolsState` and live system telemetry.
    pub fn refresh(
        &self,
        state: &DevToolsState,
        collector: &mut TelemetryCollector,
        devtools_url: Option<&str>,
    ) {
        let report = format_hud_report(state, collector, devtools_url);
        self.text_view.setString(&NSString::from_str(&report));
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "byte counts are scaled to GiB/MiB in f64 for display"
)]
fn format_hud_report(
    state: &DevToolsState,
    collector: &mut TelemetryCollector,
    devtools_url: Option<&str>,
) -> String {
    let telemetry = collector.collect(Some(std::process::id()), Duration::ZERO);
    let profile = state.profiler.summarize();
    let net = state.network.summary();
    let entries = state.debug.entries();

    let mut report = String::new();
    let _ = writeln!(
        report,
        "=== FERRO DEVTOOLS ({} | {}) ===",
        state.debug.app_name(),
        state.debug.app_id()
    );
    if let Some(url) = devtools_url {
        let _ = writeln!(report, "DevTools Endpoint : {url}");
    }
    let _ = writeln!(
        report,
        "Hot Reloads       : {}   | Breakpoint Paused: {}",
        state.reload_requests,
        state.debug.is_paused()
    );

    append_telemetry_section(&mut report, &telemetry);

    let _ = writeln!(
        report,
        "\n--- PROFILER & NETWORK ---\nFrames            : {} total, {} dropped (> {} us), avg {:.1} us ({:.1} FPS)\nNetwork Inspector : {} reqs ({} failed), avg {:.1} ms",
        profile.frames.total_frames,
        profile.frames.dropped_frames,
        profile.frames.budget_us,
        profile.frames.avg_us,
        profile.frames.estimated_fps,
        net.total_requests,
        net.failed_requests,
        net.avg_latency_ms
    );
    for span in profile.spans.iter().take(6) {
        let _ = writeln!(
            report,
            "  span {:<18} x{:<4} avg {:.1} us  p95 {} us  max {} us",
            span.name, span.count, span.avg_us, span.p95_us, span.max_us
        );
    }

    if let Some(tree) = &state.ui_tree {
        let issues = tree.audit_accessibility();
        let _ = writeln!(
            report,
            "\n--- UI TREE & ACCESSIBILITY ---\nWindow `{}`       : {} nodes, {} a11y issue(s)",
            tree.window_id,
            tree.node_count(),
            issues.len()
        );
    }

    let _ = writeln!(report, "\n--- RECENT EVENT TRACE (last 12) ---");
    for entry in entries.iter().rev().take(12) {
        let _ = writeln!(
            report,
            "#{} [{} us] {} -> {:?}",
            entry.sequence, entry.duration_us, entry.event, entry.effects
        );
    }
    report
}

#[allow(
    clippy::cast_precision_loss,
    reason = "byte counts are scaled to GiB/MiB in f64 for display"
)]
fn append_telemetry_section(
    report: &mut String,
    telemetry: &ferro_devtools::SystemTelemetrySnapshot,
) {
    let mem_used_gb = telemetry.memory.used_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    let mem_total_gb = telemetry.memory.total_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    let _ = writeln!(
        report,
        "\n--- HARDWARE & SYSTEM TELEMETRY ---\nCPU ({}) : {:.1}% total ({:.1}% user, {:.1}% sys) | load {:.2} {:.2} {:.2}\nMemory            : {:.2} / {:.2} GiB ({:.1}% pressure, compressed {:.2} GiB)\nGPU ({}) : {:?}% util | in-use {:.1} MiB\nNeural Engine     : {} ({:?} cores, state={}, clients={})\nNetwork Rates     : RX {:.1} KB/s | TX {:.1} KB/s",
        telemetry.cpu.brand,
        telemetry.cpu.total_percent,
        telemetry.cpu.user_percent,
        telemetry.cpu.system_percent,
        telemetry.cpu.load_avg_1m,
        telemetry.cpu.load_avg_5m,
        telemetry.cpu.load_avg_15m,
        mem_used_gb,
        mem_total_gb,
        telemetry.memory.pressure_percent,
        telemetry.memory.compressed_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
        telemetry.gpu.name,
        telemetry.gpu.device_utilization_percent,
        telemetry.gpu.in_use_memory_bytes.unwrap_or(0) as f64 / (1024.0 * 1024.0),
        telemetry
            .neural_engine
            .architecture
            .as_deref()
            .unwrap_or("n/a"),
        telemetry.neural_engine.core_count,
        telemetry.neural_engine.state_label,
        telemetry.neural_engine.active_clients.len(),
        telemetry.network.total_rx_bytes_per_sec / 1024.0,
        telemetry.network.total_tx_bytes_per_sec / 1024.0,
    );
    if let Some(root_vol) = telemetry.storage.volumes.first() {
        let _ = writeln!(
            report,
            "Storage ({})      : {:.1}% used ({:.1} GiB free)",
            root_vol.mount_point,
            root_vol.usage_percent,
            root_vol.available_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
        );
    }
    if let Some(smart) = &telemetry.storage.ssd_health {
        let _ = writeln!(
            report,
            "SSD SMART         : {} | wear {}% | temp {:?} C",
            smart.smart_status.as_deref().unwrap_or("unknown"),
            smart.percentage_used.unwrap_or(0),
            smart.temperature_celsius.map(|t| (t * 10.0).round() / 10.0),
        );
    }
    if let Some(proc_info) = &telemetry.target_process {
        let _ = writeln!(
            report,
            "App Process (#{}) : CPU {:.1}% | RSS {:.1} MiB | FDs {:?} | Sockets {:?}",
            proc_info.pid,
            proc_info.cpu_percent,
            proc_info.memory_rss_bytes as f64 / (1024.0 * 1024.0),
            proc_info.open_files,
            proc_info.open_sockets
        );
    }
}
