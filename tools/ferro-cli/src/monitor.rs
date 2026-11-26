//! System, hardware, and project resource monitor (`CPU`, `Memory`, `GPU`, `Neural Engine`,
//! `Network`, and `SSD/Storage`).

use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use clap::{Args, ValueEnum};
use ferro_devtools::{SystemTelemetrySnapshot, TelemetryCollector, directory_size_bytes};
use serde::Serialize;

use crate::cargo_target_directory;

/// Resource sections selectable in `ferro monitor`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum MonitorSection {
    All,
    Cpu,
    Memory,
    Gpu,
    NeuralEngine,
    Network,
    Storage,
}

#[derive(Args, Debug)]
pub(crate) struct MonitorArgs {
    /// Resource section to display.
    #[arg(long, value_enum, default_value_t = MonitorSection::All)]
    section: MonitorSection,
    /// Sampling interval in milliseconds between telemetry frames.
    #[arg(long, default_value_t = 600)]
    interval_ms: u64,
    /// Number of samples to print (`0` runs continuously until Ctrl+C).
    #[arg(long, default_value_t = 1)]
    count: u32,
    /// Optional target process ID to inspect alongside system telemetry.
    #[arg(long)]
    pid: Option<u32>,
    /// Optional Ferro project path for calculating target/bundle/storage disk footprints.
    #[arg(long)]
    project: Option<PathBuf>,
    /// Emit machine-readable JSON instead of formatted tables.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Serialize)]
struct ProjectStorageFootprint {
    project_path: String,
    target_dir_bytes: u64,
    dist_dir_bytes: u64,
    bundle_dir_bytes: u64,
}

#[derive(Debug, Serialize)]
struct MonitorReport {
    snapshot: SystemTelemetrySnapshot,
    project_storage: Option<ProjectStorageFootprint>,
}

pub(crate) fn run(args: &MonitorArgs) -> Result<(), String> {
    let mut collector = TelemetryCollector::new();
    let sample_window = Duration::from_millis(args.interval_ms);
    let project_storage = args.project.as_deref().map(measure_project_storage);
    let mut iteration = 0_u32;

    loop {
        let snapshot = collector.collect(args.pid, sample_window);
        if args.json {
            let payload = MonitorReport {
                snapshot,
                project_storage: project_storage.clone(),
            };
            let json = serde_json::to_string_pretty(&payload)
                .map_err(|error| format!("could not encode telemetry JSON: {error}"))?;
            println!("{json}");
        } else {
            if args.count == 0 && iteration > 0 {
                println!("\n────────────────────────────────────────────────────────");
            }
            print_snapshot(args.section, &snapshot, project_storage.as_ref());
        }

        iteration += 1;
        if args.count != 0 && iteration >= args.count {
            break;
        }
        thread::sleep(sample_window.max(Duration::from_millis(200)));
    }
    Ok(())
}

fn measure_project_storage(project: &Path) -> ProjectStorageFootprint {
    let target_dir = cargo_target_directory(project);
    let dist_dir = project.join("dist");
    let bundle_dir = target_dir.join("ferro");
    ProjectStorageFootprint {
        project_path: project.display().to_string(),
        target_dir_bytes: directory_size_bytes(&target_dir),
        dist_dir_bytes: directory_size_bytes(&dist_dir),
        bundle_dir_bytes: directory_size_bytes(&bundle_dir),
    }
}

impl Clone for ProjectStorageFootprint {
    fn clone(&self) -> Self {
        Self {
            project_path: self.project_path.clone(),
            target_dir_bytes: self.target_dir_bytes,
            dist_dir_bytes: self.dist_dir_bytes,
            bundle_dir_bytes: self.bundle_dir_bytes,
        }
    }
}

fn print_snapshot(
    section: MonitorSection,
    snap: &SystemTelemetrySnapshot,
    project_storage: Option<&ProjectStorageFootprint>,
) {
    println!(
        "Ferro System Monitor — {} ({}/{})",
        snap.hostname, snap.os, snap.arch
    );

    if matches!(section, MonitorSection::All | MonitorSection::Cpu) {
        print_cpu(snap);
    }
    if matches!(section, MonitorSection::All | MonitorSection::Memory) {
        print_memory(snap);
    }
    if matches!(section, MonitorSection::All | MonitorSection::Gpu) {
        print_gpu(snap);
    }
    if matches!(section, MonitorSection::All | MonitorSection::NeuralEngine) {
        print_ane(snap);
    }
    if matches!(section, MonitorSection::All | MonitorSection::Network) {
        print_network(snap);
    }
    if matches!(section, MonitorSection::All | MonitorSection::Storage) {
        print_storage(snap, project_storage);
    }
    if let Some(proc_info) = &snap.target_process {
        println!("\n[Target Process #{}]", proc_info.pid);
        println!("  Command: {}  State: {}", proc_info.name, proc_info.state);
        println!(
            "  CPU: {:.1}%  RSS: {}  VSZ: {}  FDs: {}  Sockets: {}",
            proc_info.cpu_percent,
            format_bytes(proc_info.memory_rss_bytes),
            format_bytes(proc_info.memory_vsz_bytes),
            fmt_opt(proc_info.open_files),
            fmt_opt(proc_info.open_sockets)
        );
    }
}

fn fmt_opt<T: std::fmt::Display>(value: Option<T>) -> String {
    value.map_or_else(|| "n/a".to_owned(), |v| v.to_string())
}

fn fmt_opt_pct(value: Option<f64>) -> String {
    value.map_or_else(|| "n/a".to_owned(), |v| format!("{v:.1}%"))
}

fn print_cpu(snap: &SystemTelemetrySnapshot) {
    println!("\n[CPU] {}", snap.cpu.brand);
    println!(
        "  Usage: {:.1}% total ({:.1}% user, {:.1}% sys, {:.1}% idle) | Source: {}",
        snap.cpu.total_percent,
        snap.cpu.user_percent,
        snap.cpu.system_percent,
        snap.cpu.idle_percent,
        snap.cpu.source
    );
    println!(
        "  Cores: {} physical / {} logical (P: {}, E: {}) | Load Avg: {:.2} {:.2} {:.2}",
        snap.cpu.physical_cores,
        snap.cpu.logical_cores,
        fmt_opt(snap.cpu.performance_cores),
        fmt_opt(snap.cpu.efficiency_cores),
        snap.cpu.load_avg_1m,
        snap.cpu.load_avg_5m,
        snap.cpu.load_avg_15m
    );
}

fn print_memory(snap: &SystemTelemetrySnapshot) {
    let m = &snap.memory;
    println!("\n[Memory]");
    println!(
        "  Used: {} / {} ({:.1}% pressure) | Available: {}",
        format_bytes(m.used_bytes),
        format_bytes(m.total_bytes),
        m.pressure_percent,
        format_bytes(m.available_bytes)
    );
    println!(
        "  Breakdown: active {} | wired {} | compressed {} | inactive {} | free {}",
        format_bytes(m.active_bytes),
        format_bytes(m.wired_bytes),
        format_bytes(m.compressed_bytes),
        format_bytes(m.inactive_bytes),
        format_bytes(m.free_bytes)
    );
    println!(
        "  Swap: {} / {} (free {}) | Pageins: {} | Pageouts: {}",
        format_bytes(m.swap_used_bytes),
        format_bytes(m.swap_total_bytes),
        format_bytes(m.swap_free_bytes),
        m.pageins,
        m.pageouts
    );
}

fn print_gpu(snap: &SystemTelemetrySnapshot) {
    let g = &snap.gpu;
    println!("\n[GPU] {}", g.name);
    if !g.available {
        println!("  Status: unavailable on this platform");
        return;
    }
    println!(
        "  Cores: {} | Driver: {} | Power: {}",
        fmt_opt(g.core_count),
        g.driver.as_deref().unwrap_or("unknown"),
        g.power_state.as_deref().unwrap_or("unknown")
    );
    println!(
        "  Utilization: device {} | renderer {} | tiler {}",
        fmt_opt_pct(g.device_utilization_percent),
        fmt_opt_pct(g.renderer_utilization_percent),
        fmt_opt_pct(g.tiler_utilization_percent)
    );
    println!(
        "  Memory: in-use {} | allocated {} | PB {} | recoveries {}",
        g.in_use_memory_bytes
            .map_or_else(|| "n/a".to_owned(), format_bytes),
        g.allocated_memory_bytes
            .map_or_else(|| "n/a".to_owned(), format_bytes),
        g.parameter_buffer_bytes
            .map_or_else(|| "n/a".to_owned(), format_bytes),
        fmt_opt(g.recovery_count)
    );
}

fn print_ane(snap: &SystemTelemetrySnapshot) {
    let ane = &snap.neural_engine;
    println!("\n[Neural Engine (ANE)]");
    if !ane.available {
        println!(
            "  Status: {} ({})",
            ane.state_label,
            ane.note.as_deref().unwrap_or("unavailable")
        );
        return;
    }
    println!(
        "  Arch: {} | Cores: {} | Units: {} | State: {} (power {}/{})",
        ane.architecture.as_deref().unwrap_or("unknown"),
        fmt_opt(ane.core_count),
        fmt_opt(ane.unit_count),
        ane.state_label,
        fmt_opt(ane.power_state),
        fmt_opt(ane.max_power_state)
    );
    if let Some(mw) = ane.power_mw {
        println!("  ANE Power: {mw:.1} mW");
    }
    if !ane.active_clients.is_empty() {
        println!("  ANE Daemons/Clients:");
        for client in &ane.active_clients {
            println!(
                "    - pid {:<6} CPU {:>5.1}%  RSS {:<9} {}",
                client.pid,
                client.cpu_percent,
                format_bytes(client.memory_rss_bytes),
                client.name
            );
        }
    }
    if let Some(note) = &ane.note {
        println!("  Note: {note}");
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "non-negative throughput rates convert cleanly for byte formatting"
)]
fn print_network(snap: &SystemTelemetrySnapshot) {
    let n = &snap.network;
    println!("\n[Network Usage]");
    println!(
        "  Throughput: RX {}/s | TX {}/s  (Cumulative RX {} | TX {})",
        format_bytes(n.total_rx_bytes_per_sec.max(0.0) as u64),
        format_bytes(n.total_tx_bytes_per_sec.max(0.0) as u64),
        format_bytes(n.total_rx_bytes),
        format_bytes(n.total_tx_bytes)
    );
    for iface in n
        .interfaces
        .iter()
        .filter(|item| item.rx_bytes > 0 || item.tx_bytes > 0)
        .take(6)
    {
        println!(
            "  {:<8} mtu {:<5} RX {}/s ({})  TX {}/s ({})",
            iface.name,
            iface.mtu,
            format_bytes(iface.rx_bytes_per_sec.max(0.0) as u64),
            format_bytes(iface.rx_bytes),
            format_bytes(iface.tx_bytes_per_sec.max(0.0) as u64),
            format_bytes(iface.tx_bytes),
        );
    }
    if !n.top_processes.is_empty() {
        println!("  Top Network Processes:");
        for proc_net in n.top_processes.iter().take(5) {
            println!(
                "    - {:<20} (pid {:<6}) RX {}  TX {}",
                proc_net.name,
                proc_net.pid,
                format_bytes(proc_net.rx_bytes),
                format_bytes(proc_net.tx_bytes)
            );
        }
    }
}

fn print_storage(
    snap: &SystemTelemetrySnapshot,
    project_storage: Option<&ProjectStorageFootprint>,
) {
    let s = &snap.storage;
    println!("\n[SSD / Storage Usage]");
    for vol in &s.volumes {
        println!(
            "  {:<22} {:>5.1}% used  ({} free / {} total)  [{}]",
            vol.mount_point,
            vol.usage_percent,
            format_bytes(vol.available_bytes),
            format_bytes(vol.total_bytes),
            vol.filesystem
        );
    }
    for io in &s.io_stats {
        println!(
            "  I/O {:<10} read {:.2} MB/s | write {:.2} MB/s | {:.0} IOPS",
            io.device, io.read_mb_per_sec, io.write_mb_per_sec, io.iops
        );
    }
    if let Some(smart) = &s.ssd_health {
        println!(
            "  SSD SMART: {} ({}) | status {} | wear {}% | spare {}% | temp {}",
            smart.device_name.as_deref().unwrap_or("SSD"),
            smart.protocol.as_deref().unwrap_or("unknown"),
            smart.smart_status.as_deref().unwrap_or("unknown"),
            smart.percentage_used.unwrap_or(0),
            smart.available_spare_percent.unwrap_or(0),
            smart
                .temperature_celsius
                .map_or_else(|| "n/a".to_owned(), |temp| format!("{temp:.1} C"))
        );
        if let (Some(read_b), Some(write_b)) =
            (smart.bytes_read_lifetime, smart.bytes_written_lifetime)
        {
            println!(
                "  SSD Lifetime: read {} | written {} | hours {} | cycles {} | media_err {}",
                format_bytes(read_b),
                format_bytes(write_b),
                fmt_opt(smart.power_on_hours),
                fmt_opt(smart.power_cycles),
                fmt_opt(smart.media_errors)
            );
        }
    }
    if let Some(proj) = project_storage {
        println!(
            "  Project Footprint ({}): target/ {} | bundles {} | dist/ {}",
            proj.project_path,
            format_bytes(proj.target_dir_bytes),
            format_bytes(proj.bundle_dir_bytes),
            format_bytes(proj.dist_dir_bytes)
        );
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "byte counts are formatted cleanly in KiB/MiB/GiB/TiB"
)]
pub(crate) fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    const TIB: f64 = GIB * 1024.0;
    let b = bytes as f64;
    if b >= TIB {
        format!("{:.2} TiB", b / TIB)
    } else if b >= GIB {
        format!("{:.2} GiB", b / GIB)
    } else if b >= MIB {
        format!("{:.1} MiB", b / MIB)
    } else if b >= KIB {
        format!("{:.1} KiB", b / KIB)
    } else {
        format!("{bytes} B")
    }
}
