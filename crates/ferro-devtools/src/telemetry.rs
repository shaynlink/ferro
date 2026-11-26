//! Cross-platform hardware, OS, and process telemetry collectors (CPU, Memory, GPU, Neural
//! Engine, Network Usage, and SSD/Storage Usage).

use std::collections::{BTreeMap, HashMap};
use std::process::Command;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::time::Instant;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Lightweight process sample used in telemetry breakdowns (e.g. ANE clients).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProcessSample {
    /// OS process identifier.
    pub pid: u32,
    /// Executable or command name.
    pub name: String,
    /// CPU utilization percentage (`0.0..=100.0 * cores`).
    pub cpu_percent: f64,
    /// Resident set size in bytes.
    pub memory_rss_bytes: u64,
}

/// CPU topology, load averages, and utilization percentages.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CpuTelemetry {
    /// CPU brand or model string.
    pub brand: String,
    /// Total logical CPU cores.
    pub logical_cores: usize,
    /// Physical CPU cores.
    pub physical_cores: usize,
    /// Apple Silicon / heterogeneous performance-core count if available.
    pub performance_cores: Option<usize>,
    /// Apple Silicon / heterogeneous efficiency-core count if available.
    pub efficiency_cores: Option<usize>,
    /// User-space CPU utilization percentage.
    pub user_percent: f64,
    /// Kernel/system CPU utilization percentage.
    pub system_percent: f64,
    /// Idle CPU percentage.
    pub idle_percent: f64,
    /// Combined active CPU utilization percentage (`user + system`).
    pub total_percent: f64,
    /// 1-minute system load average.
    pub load_avg_1m: f64,
    /// 5-minute system load average.
    pub load_avg_5m: f64,
    /// 15-minute system load average.
    pub load_avg_15m: f64,
    /// Probe source description.
    pub source: String,
}

/// Physical and virtual memory breakdown.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MemoryTelemetry {
    /// Total installed physical RAM in bytes.
    pub total_bytes: u64,
    /// Used physical memory (`active + wired + compressed`) in bytes.
    pub used_bytes: u64,
    /// Free + reclaimable memory in bytes.
    pub available_bytes: u64,
    /// Completely unused pages in bytes.
    pub free_bytes: u64,
    /// Active memory in bytes.
    pub active_bytes: u64,
    /// Inactive memory in bytes.
    pub inactive_bytes: u64,
    /// Wired (non-pageable kernel/driver) memory in bytes.
    pub wired_bytes: u64,
    /// Memory occupied by VM compressor in bytes.
    pub compressed_bytes: u64,
    /// Purgeable memory in bytes.
    pub purgeable_bytes: u64,
    /// Total swap space in bytes.
    pub swap_total_bytes: u64,
    /// Used swap space in bytes.
    pub swap_used_bytes: u64,
    /// Free swap space in bytes.
    pub swap_free_bytes: u64,
    /// Kernel page size in bytes.
    pub page_size: u64,
    /// Cumulative page-ins count.
    pub pageins: u64,
    /// Cumulative page-outs count.
    pub pageouts: u64,
    /// Estimated memory pressure percentage (`0.0..=100.0`).
    pub pressure_percent: f64,
}

/// GPU utilization and memory statistics.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GpuTelemetry {
    /// Whether a GPU device telemetry source was detected.
    pub available: bool,
    /// GPU model name (e.g. `Apple M3 Max`).
    pub name: String,
    /// GPU execution core count if exposed.
    pub core_count: Option<u32>,
    /// Metal plugin or driver identifier.
    pub driver: Option<String>,
    /// Overall GPU device utilization percentage (`0.0..=100.0`).
    pub device_utilization_percent: Option<f64>,
    /// GPU shader/renderer utilization percentage.
    pub renderer_utilization_percent: Option<f64>,
    /// GPU tiler utilization percentage.
    pub tiler_utilization_percent: Option<f64>,
    /// Currently active GPU system memory in bytes.
    pub in_use_memory_bytes: Option<u64>,
    /// Total allocated GPU system memory in bytes.
    pub allocated_memory_bytes: Option<u64>,
    /// Allocated parameter buffer size in bytes.
    pub parameter_buffer_bytes: Option<u64>,
    /// GPU recovery/restart counter.
    pub recovery_count: Option<u64>,
    /// Current driver power state if reported.
    pub power_state: Option<String>,
}

/// Apple Neural Engine (ANE) / NPU hardware and activity telemetry.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NeuralEngineTelemetry {
    /// Whether Neural Engine hardware was detected.
    pub available: bool,
    /// ANE architecture string (e.g. `h15g`).
    pub architecture: Option<String>,
    /// Number of ANE cores (e.g. `16`).
    pub core_count: Option<u32>,
    /// Number of ANE units (`ANEDevicePropertyNumANEs`).
    pub unit_count: Option<u32>,
    /// Current hardware power state reported by `IOPowerManagement`.
    pub power_state: Option<u32>,
    /// Maximum hardware power state reported by `IOPowerManagement`.
    pub max_power_state: Option<u32>,
    /// Human-readable activity label (`active`, `idle`, `unavailable`).
    pub state_label: String,
    /// Instantaneous ANE power draw in milliwatts if elevated `powermetrics` succeeded.
    pub power_mw: Option<f64>,
    /// Active ANE system daemons/clients (`aned`, `aneuserd`, `ANECompilerService`).
    pub active_clients: Vec<ProcessSample>,
    /// Explanatory note on probe capabilities and permissions.
    pub note: Option<String>,
}

/// Per-interface network traffic counters and rates.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NetworkInterfaceTelemetry {
    /// Interface name (e.g. `en0`, `lo0`, `eth0`).
    pub name: String,
    /// Maximum transmission unit.
    pub mtu: u64,
    /// Cumulative received bytes.
    pub rx_bytes: u64,
    /// Cumulative transmitted bytes.
    pub tx_bytes: u64,
    /// Cumulative received packets.
    pub rx_packets: u64,
    /// Cumulative transmitted packets.
    pub tx_packets: u64,
    /// Cumulative receive errors.
    pub rx_errors: u64,
    /// Cumulative transmit errors.
    pub tx_errors: u64,
    /// Measured receive rate in bytes/sec across the sample window.
    pub rx_bytes_per_sec: f64,
    /// Measured transmit rate in bytes/sec across the sample window.
    pub tx_bytes_per_sec: f64,
}

/// Per-process network traffic summary.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProcessNetworkUsage {
    /// Process identifier.
    pub pid: u32,
    /// Process name.
    pub name: String,
    /// Cumulative bytes received.
    pub rx_bytes: u64,
    /// Cumulative bytes transmitted.
    pub tx_bytes: u64,
    /// Receive throughput in bytes/sec when delta is available.
    pub rx_bytes_per_sec: f64,
    /// Transmit throughput in bytes/sec when delta is available.
    pub tx_bytes_per_sec: f64,
}

/// Aggregate network throughput and interface statistics.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NetworkUsageTelemetry {
    /// Per-interface counters and rates.
    pub interfaces: Vec<NetworkInterfaceTelemetry>,
    /// Aggregate non-loopback received bytes.
    pub total_rx_bytes: u64,
    /// Aggregate non-loopback transmitted bytes.
    pub total_tx_bytes: u64,
    /// Aggregate non-loopback receive throughput in bytes/sec.
    pub total_rx_bytes_per_sec: f64,
    /// Aggregate non-loopback transmit throughput in bytes/sec.
    pub total_tx_bytes_per_sec: f64,
    /// Top processes ordered by total traffic.
    pub top_processes: Vec<ProcessNetworkUsage>,
}

/// Mounted filesystem volume usage.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StorageVolumeTelemetry {
    /// Filesystem device or volume identifier.
    pub filesystem: String,
    /// Mount point path.
    pub mount_point: String,
    /// Total capacity in bytes.
    pub total_bytes: u64,
    /// Used bytes.
    pub used_bytes: u64,
    /// Available bytes for unprivileged users.
    pub available_bytes: u64,
    /// Usage percentage (`0.0..=100.0`).
    pub usage_percent: f64,
}

/// Block device instantaneous I/O rate.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DiskIoTelemetry {
    /// Disk device name (e.g. `disk0`, `nvme0n1`).
    pub device: String,
    /// Read throughput in MB/sec.
    pub read_mb_per_sec: f64,
    /// Write throughput in MB/sec.
    pub write_mb_per_sec: f64,
    /// Total operations/sec (IOPS).
    pub iops: f64,
}

/// `NVMe` / Apple SSD SMART health and lifetime wear metrics.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SsdHealthTelemetry {
    /// Physical SSD device name (`APPLE SSD ...`).
    pub device_name: Option<String>,
    /// Hardware interface protocol (`Apple Fabric`, `NVMe`, etc.).
    pub protocol: Option<String>,
    /// SMART verification status (`Verified`, etc.).
    pub smart_status: Option<String>,
    /// Whether the underlying medium is solid-state.
    pub solid_state: bool,
    /// Reported SSD controller temperature in degrees Celsius.
    pub temperature_celsius: Option<f64>,
    /// Estimated percentage of SSD rated endurance consumed (`0..=100`).
    pub percentage_used: Option<u64>,
    /// Remaining spare block percentage (`0..=100`).
    pub available_spare_percent: Option<u64>,
    /// Lifetime bytes read reported by `NVMe` SMART (`data_units * 512_000`).
    pub bytes_read_lifetime: Option<u64>,
    /// Lifetime bytes written reported by `NVMe` SMART (`data_units * 512_000`).
    pub bytes_written_lifetime: Option<u64>,
    /// Cumulative power-on hours.
    pub power_on_hours: Option<u64>,
    /// Cumulative power cycles.
    pub power_cycles: Option<u64>,
    /// Count of media errors.
    pub media_errors: Option<u64>,
    /// Count of unsafe shutdowns.
    pub unsafe_shutdowns: Option<u64>,
}

/// Storage volumes, live disk I/O throughput, and SSD SMART health.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StorageTelemetry {
    /// Mounted filesystem volumes.
    pub volumes: Vec<StorageVolumeTelemetry>,
    /// Live block-device read/write throughput.
    pub io_stats: Vec<DiskIoTelemetry>,
    /// Root SSD SMART/wear health when available.
    pub ssd_health: Option<SsdHealthTelemetry>,
}

/// Resource footprint for one specific target process (`--pid`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProcessTelemetry {
    /// Process identifier.
    pub pid: u32,
    /// Command name or executable path.
    pub name: String,
    /// Process CPU percentage.
    pub cpu_percent: f64,
    /// Resident memory in bytes.
    pub memory_rss_bytes: u64,
    /// Virtual memory size in bytes.
    pub memory_vsz_bytes: u64,
    /// OS process state string.
    pub state: String,
    /// Open file descriptor count if inspected.
    pub open_files: Option<usize>,
    /// Open network socket count if inspected.
    pub open_sockets: Option<usize>,
}

/// Complete multi-domain system telemetry snapshot.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SystemTelemetrySnapshot {
    /// Collection timestamp in Unix epoch milliseconds.
    pub timestamp_ms: u64,
    /// Operating system identifier (`macos`, `linux`, `windows`).
    pub os: String,
    /// CPU architecture (`aarch64`, `x86_64`, etc.).
    pub arch: String,
    /// Hostname reported by the system.
    pub hostname: String,
    /// CPU telemetry.
    pub cpu: CpuTelemetry,
    /// Memory and swap telemetry.
    pub memory: MemoryTelemetry,
    /// GPU telemetry.
    pub gpu: GpuTelemetry,
    /// Neural Engine telemetry.
    pub neural_engine: NeuralEngineTelemetry,
    /// Network usage telemetry.
    pub network: NetworkUsageTelemetry,
    /// SSD and storage telemetry.
    pub storage: StorageTelemetry,
    /// Optional target application process telemetry.
    pub target_process: Option<ProcessTelemetry>,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[derive(Clone, Debug)]
struct NetBaseline {
    captured_at: Instant,
    interfaces: BTreeMap<String, (u64, u64)>,
    #[cfg(target_os = "macos")]
    processes: BTreeMap<u32, (u64, u64)>,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Debug)]
struct DiskBaseline {
    captured_at: Instant,
    read_bytes: u64,
    write_bytes: u64,
    read_ops: u64,
    write_ops: u64,
}

/// Stateful telemetry collector that computes accurate network and disk I/O throughput deltas.
#[derive(Debug, Default)]
pub struct TelemetryCollector {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    prev_net: Option<NetBaseline>,
    #[cfg(target_os = "macos")]
    prev_disk: Option<DiskBaseline>,
    #[cfg(target_os = "linux")]
    prev_cpu: Option<LinuxCpuTicks>,
}

impl TelemetryCollector {
    /// Creates a fresh collector with no cached rate baseline.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Collects a full system telemetry snapshot.
    ///
    /// When `sample_window` is non-zero and no prior baseline exists, samples interface and block
    /// counters across `sample_window` so throughput rates are populated immediately.
    pub fn collect(
        &mut self,
        target_pid: Option<u32>,
        sample_window: Duration,
    ) -> SystemTelemetrySnapshot {
        collect_platform_snapshot(self, target_pid, sample_window)
    }
}

#[cfg(target_os = "macos")]
#[allow(
    clippy::cast_precision_loss,
    reason = "byte deltas and millisecond intervals fit safely in f64 for throughput display"
)]
fn collect_platform_snapshot(
    collector: &mut TelemetryCollector,
    target_pid: Option<u32>,
    sample_window: Duration,
) -> SystemTelemetrySnapshot {
    let needs_baseline = collector.prev_net.is_none() || collector.prev_disk.is_none();
    if needs_baseline && !sample_window.is_zero() {
        let now = Instant::now();
        if let Some(net_output) = run_command_stdout("netstat", &["-ibn"]) {
            let parsed = parse_netstat_ibn(&net_output);
            let interfaces = parsed
                .into_iter()
                .map(|iface| (iface.name, (iface.rx_bytes, iface.tx_bytes)))
                .collect();
            let processes =
                run_command_stdout("nettop", &["-P", "-L", "1", "-J", "bytes_in,bytes_out"])
                    .map_or_default(|raw| {
                        parse_nettop_csv(&raw)
                            .into_iter()
                            .map(|proc_net| (proc_net.pid, (proc_net.rx_bytes, proc_net.tx_bytes)))
                            .collect()
                    });
            collector.prev_net = Some(NetBaseline {
                captured_at: now,
                interfaces,
                processes,
            });
        }
        if let Some(block_ioreg) = run_command_stdout(
            "ioreg",
            &["-r", "-c", "IOBlockStorageDriver", "-k", "Statistics"],
        ) && let Some((rb, wb, ro, wo)) = parse_ioreg_block_storage_stats(&block_ioreg)
        {
            collector.prev_disk = Some(DiskBaseline {
                captured_at: now,
                read_bytes: rb,
                write_bytes: wb,
                read_ops: ro,
                write_ops: wo,
            });
        }
        if sample_window <= Duration::from_millis(800) {
            std::thread::sleep(sample_window);
        }
    }

    // `top -l 2 -n 0 -s 0` takes ~1s and gives accurate delta CPU user/sys/idle percentages.
    let cpu = collect_macos_cpu(sample_window > Duration::from_millis(50));
    let memory = collect_macos_memory();
    let gpu = collect_macos_gpu();
    let neural_engine = collect_macos_ane();
    let network = collect_macos_network(collector);
    let storage = collect_macos_storage(collector);
    let target_process = target_pid.and_then(collect_unix_process);

    SystemTelemetrySnapshot {
        timestamp_ms: now_ms(),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        hostname: detect_hostname(),
        cpu,
        memory,
        gpu,
        neural_engine,
        network,
        storage,
        target_process,
    }
}

#[cfg(target_os = "macos")]
fn collect_macos_cpu(use_two_sample_top: bool) -> CpuTelemetry {
    let brand =
        sysctl_string("machdep.cpu.brand_string").unwrap_or_else(|| "Apple Silicon".to_owned());
    let logical_cores = sysctl_u64("hw.logicalcpu")
        .or_else(|| sysctl_u64("hw.ncpu"))
        .map_or(1, |val| usize::try_from(val).unwrap_or(1));
    let physical_cores = sysctl_u64("hw.physicalcpu").map_or(logical_cores, |val| {
        usize::try_from(val).unwrap_or(logical_cores)
    });
    let performance_cores =
        sysctl_u64("hw.perflevel0.physicalcpu").and_then(|val| usize::try_from(val).ok());
    let efficiency_cores =
        sysctl_u64("hw.perflevel1.physicalcpu").and_then(|val| usize::try_from(val).ok());
    let (load_one, load_five, load_fifteen) =
        sysctl_string("vm.loadavg").map_or((0.0, 0.0, 0.0), |raw| parse_sysctl_loadavg(&raw));

    let args: &[&str] = if use_two_sample_top {
        &["-l", "2", "-n", "0", "-s", "0"]
    } else {
        &["-l", "1", "-n", "0"]
    };
    let (user_percent, system_percent, idle_percent, source) = run_command_stdout("top", args)
        .and_then(|text| parse_top_cpu_output(&text))
        .map_or_else(
            || (0.0, 0.0, 100.0, "sysctl-loadavg".to_owned()),
            |(user, sys, idle)| (user, sys, idle, "top+sysctl".to_owned()),
        );

    CpuTelemetry {
        brand,
        logical_cores,
        physical_cores,
        performance_cores,
        efficiency_cores,
        user_percent,
        system_percent,
        idle_percent,
        total_percent: (user_percent + system_percent).clamp(0.0, 100.0),
        load_avg_1m: load_one,
        load_avg_5m: load_five,
        load_avg_15m: load_fifteen,
        source,
    }
}

#[cfg(target_os = "macos")]
fn collect_macos_memory() -> MemoryTelemetry {
    let total_bytes = sysctl_u64("hw.memsize").unwrap_or(0);
    let (swap_total_bytes, swap_used_bytes, swap_free_bytes) =
        sysctl_string("vm.swapusage").map_or((0, 0, 0), |raw| parse_sysctl_swapusage(&raw));
    let mut mem =
        run_command_stdout("vm_stat", &[]).map_or_default(|raw| parse_vm_stat(&raw, total_bytes));
    mem.total_bytes = total_bytes;
    mem.swap_total_bytes = swap_total_bytes;
    mem.swap_used_bytes = swap_used_bytes;
    mem.swap_free_bytes = swap_free_bytes;
    mem
}

#[cfg(target_os = "macos")]
fn collect_macos_gpu() -> GpuTelemetry {
    let Some(raw) = run_command_stdout("ioreg", &["-r", "-d", "1", "-c", "IOAccelerator"]) else {
        return GpuTelemetry::default();
    };
    parse_ioreg_gpu(&raw)
}

#[cfg(target_os = "macos")]
fn collect_macos_ane() -> NeuralEngineTelemetry {
    let h11 = run_command_stdout("ioreg", &["-r", "-d", "3", "-c", "H11ANEIn"]).unwrap_or_default();
    let lb = run_command_stdout("ioreg", &["-r", "-d", "3", "-c", "H1xANELoadBalancer"])
        .unwrap_or_default();
    let mut ane = parse_ioreg_ane(&h11, &lb);
    ane.active_clients = collect_ane_client_processes();
    if let Some(power_mw) = try_powermetrics_ane_mw() {
        ane.power_mw = Some(power_mw);
        if power_mw > 5.0 {
            "active".clone_into(&mut ane.state_label);
        }
    } else if ane.available && ane.note.is_none() {
        ane.note = Some(
            "IORegistry + ANE process probe (run with passwordless sudo for ANE milliwatt powermetrics)"
                .to_owned(),
        );
    }
    ane
}

#[cfg(target_os = "macos")]
#[allow(
    clippy::cast_precision_loss,
    reason = "byte deltas and elapsed seconds are computed in f64 for throughput rates"
)]
fn collect_macos_network(collector: &mut TelemetryCollector) -> NetworkUsageTelemetry {
    let now = Instant::now();
    let mut interfaces =
        run_command_stdout("netstat", &["-ibn"]).map_or_default(|raw| parse_netstat_ibn(&raw));
    let mut top_processes =
        run_command_stdout("nettop", &["-P", "-L", "1", "-J", "bytes_in,bytes_out"])
            .map_or_default(|raw| parse_nettop_csv(&raw));

    if let Some(prev) = &collector.prev_net {
        let elapsed_secs = now.duration_since(prev.captured_at).as_secs_f64();
        if elapsed_secs > 0.01 {
            for iface in &mut interfaces {
                if let Some(&(prev_rx, prev_tx)) = prev.interfaces.get(&iface.name) {
                    iface.rx_bytes_per_sec =
                        (iface.rx_bytes.saturating_sub(prev_rx) as f64) / elapsed_secs;
                    iface.tx_bytes_per_sec =
                        (iface.tx_bytes.saturating_sub(prev_tx) as f64) / elapsed_secs;
                }
            }
            for proc_net in &mut top_processes {
                if let Some(&(prev_rx, prev_tx)) = prev.processes.get(&proc_net.pid) {
                    proc_net.rx_bytes_per_sec =
                        (proc_net.rx_bytes.saturating_sub(prev_rx) as f64) / elapsed_secs;
                    proc_net.tx_bytes_per_sec =
                        (proc_net.tx_bytes.saturating_sub(prev_tx) as f64) / elapsed_secs;
                }
            }
        }
    }

    collector.prev_net = Some(NetBaseline {
        captured_at: now,
        interfaces: interfaces
            .iter()
            .map(|iface| (iface.name.clone(), (iface.rx_bytes, iface.tx_bytes)))
            .collect(),
        processes: top_processes
            .iter()
            .map(|proc_net| (proc_net.pid, (proc_net.rx_bytes, proc_net.tx_bytes)))
            .collect(),
    });

    let mut rx_bytes_sum = 0_u64;
    let mut tx_bytes_sum = 0_u64;
    let mut rx_rate_sum = 0.0_f64;
    let mut tx_rate_sum = 0.0_f64;
    for iface in &interfaces {
        if iface.name.starts_with("lo") {
            continue;
        }
        rx_bytes_sum = rx_bytes_sum.saturating_add(iface.rx_bytes);
        tx_bytes_sum = tx_bytes_sum.saturating_add(iface.tx_bytes);
        rx_rate_sum += iface.rx_bytes_per_sec;
        tx_rate_sum += iface.tx_bytes_per_sec;
    }

    top_processes
        .sort_by_key(|item| std::cmp::Reverse(item.rx_bytes.saturating_add(item.tx_bytes)));
    top_processes.truncate(10);

    NetworkUsageTelemetry {
        interfaces,
        total_rx_bytes: rx_bytes_sum,
        total_tx_bytes: tx_bytes_sum,
        total_rx_bytes_per_sec: rx_rate_sum,
        total_tx_bytes_per_sec: tx_rate_sum,
        top_processes,
    }
}

#[cfg(target_os = "macos")]
#[allow(
    clippy::cast_precision_loss,
    reason = "block byte deltas and elapsed seconds are computed in f64 for MB/s rates"
)]
fn collect_macos_storage(collector: &mut TelemetryCollector) -> StorageTelemetry {
    let volumes = run_command_stdout("df", &["-k"]).map_or_default(|raw| parse_df_k(&raw, true));

    let mut io_stats = Vec::new();
    let now = Instant::now();
    if let Some(block_ioreg) = run_command_stdout(
        "ioreg",
        &["-r", "-c", "IOBlockStorageDriver", "-k", "Statistics"],
    ) && let Some((rb, wb, ro, wo)) = parse_ioreg_block_storage_stats(&block_ioreg)
    {
        if let Some(prev) = &collector.prev_disk {
            let elapsed = now.duration_since(prev.captured_at).as_secs_f64();
            if elapsed > 0.01 {
                let read_mb =
                    (rb.saturating_sub(prev.read_bytes) as f64) / (1024.0 * 1024.0) / elapsed;
                let write_mb =
                    (wb.saturating_sub(prev.write_bytes) as f64) / (1024.0 * 1024.0) / elapsed;
                let iops = ((ro.saturating_sub(prev.read_ops) + wo.saturating_sub(prev.write_ops))
                    as f64)
                    / elapsed;
                io_stats.push(DiskIoTelemetry {
                    device: "disk0".to_owned(),
                    read_mb_per_sec: read_mb,
                    write_mb_per_sec: write_mb,
                    iops,
                });
            }
        }
        collector.prev_disk = Some(DiskBaseline {
            captured_at: now,
            read_bytes: rb,
            write_bytes: wb,
            read_ops: ro,
            write_ops: wo,
        });
    }
    if io_stats.is_empty()
        && let Some(iostat_raw) = run_command_stdout("iostat", &["-d", "-c", "1"])
    {
        io_stats = parse_iostat_output(&iostat_raw);
    }

    let ssd_health = run_command_stdout("diskutil", &["info", "-plist", "/System/Volumes/Data"])
        .or_else(|| run_command_stdout("diskutil", &["info", "-plist", "/"]))
        .map(|plist| parse_diskutil_smart_plist(&plist));

    StorageTelemetry {
        volumes,
        io_stats,
        ssd_health,
    }
}

#[cfg(target_os = "macos")]
fn collect_ane_client_processes() -> Vec<ProcessSample> {
    let Some(output) = run_command_stdout("ps", &["-Ao", "pid=,pcpu=,rss=,comm="]) else {
        return Vec::new();
    };
    let mut clients = Vec::new();
    for line in output.lines() {
        let Some(proc_sample) = parse_ps_line(line) else {
            continue;
        };
        let lower = proc_sample.name.to_ascii_lowercase();
        if lower.ends_with("/aned")
            || lower == "aned"
            || lower.contains("aneuserd")
            || lower.contains("anecompilerservice")
        {
            clients.push(proc_sample);
        }
    }
    clients
}

#[cfg(target_os = "macos")]
fn try_powermetrics_ane_mw() -> Option<f64> {
    let output = Command::new("sudo")
        .args([
            "-n",
            "powermetrics",
            "-n",
            "1",
            "-i",
            "100",
            "--samplers",
            "ane_power",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if line.contains("ANE Power:") {
            let number = line
                .split(':')
                .nth(1)?
                .split_whitespace()
                .next()?
                .parse::<f64>()
                .ok()?;
            return Some(number);
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn sysctl_string(key: &str) -> Option<String> {
    run_command_stdout("sysctl", &["-n", key]).map(|value| value.trim().to_owned())
}

#[cfg(target_os = "macos")]
fn sysctl_u64(key: &str) -> Option<u64> {
    sysctl_string(key).and_then(|value| value.parse::<u64>().ok())
}

#[cfg(unix)]
fn collect_unix_process(pid: u32) -> Option<ProcessTelemetry> {
    let raw = run_command_stdout(
        "ps",
        &[
            "-p",
            &pid.to_string(),
            "-o",
            "pid=,pcpu=,rss=,vsz=,state=,comm=",
        ],
    )?;
    let line = raw.lines().next()?.trim();
    let cols: Vec<&str> = line.split_whitespace().collect();
    if cols.len() < 6 {
        return None;
    }
    let parsed_pid = cols[0].parse::<u32>().ok()?;
    let cpu_percent = cols[1].replace(',', ".").parse::<f64>().unwrap_or(0.0);
    let rss_kb = cols[2].parse::<u64>().unwrap_or(0);
    let vsz_kb = cols[3].parse::<u64>().unwrap_or(0);
    let state = cols[4].to_owned();
    let name = cols[5..].join(" ");

    let open_files = run_command_stdout("lsof", &["-p", &pid.to_string()])
        .map(|out| out.lines().skip(1).count());
    let open_sockets = run_command_stdout("lsof", &["-i", "-n", "-P", "-p", &pid.to_string()])
        .map(|out| out.lines().skip(1).count());

    Some(ProcessTelemetry {
        pid: parsed_pid,
        name,
        cpu_percent,
        memory_rss_bytes: rss_kb * 1024,
        memory_vsz_bytes: vsz_kb * 1024,
        state,
        open_files,
        open_sockets,
    })
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug)]
struct LinuxCpuTicks {
    user: u64,
    nice: u64,
    system: u64,
    idle: u64,
    iowait: u64,
    irq: u64,
    softirq: u64,
}

#[cfg(target_os = "linux")]
fn prime_linux_baselines(collector: &mut TelemetryCollector, sample_window: Duration) {
    if collector.prev_cpu.is_some() || sample_window.is_zero() {
        return;
    }
    if let Ok(stat) = std::fs::read_to_string("/proc/stat") {
        collector.prev_cpu = parse_proc_stat(&stat);
    }
    if let Ok(net) = std::fs::read_to_string("/proc/net/dev") {
        let interfaces = parse_proc_net_dev(&net)
            .into_iter()
            .map(|iface| (iface.name, (iface.rx_bytes, iface.tx_bytes)))
            .collect();
        collector.prev_net = Some(NetBaseline {
            captured_at: Instant::now(),
            interfaces,
        });
    }
    std::thread::sleep(sample_window.min(Duration::from_millis(500)));
}

#[cfg(target_os = "linux")]
#[allow(
    clippy::cast_precision_loss,
    reason = "CPU tick deltas fit cleanly in f64 percentages"
)]
fn collect_linux_cpu(collector: &mut TelemetryCollector) -> CpuTelemetry {
    let cpu_info = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let brand = cpu_info
        .lines()
        .find_map(|line| {
            line.strip_prefix("model name")
                .and_then(|rest| rest.split(':').nth(1))
                .map(|val| val.trim().to_owned())
        })
        .unwrap_or_else(|| "Linux CPU".to_owned());
    let logical_cores = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    let (one_min, five_min, fifteen_min) =
        std::fs::read_to_string("/proc/loadavg")
            .ok()
            .map_or((0.0, 0.0, 0.0), |raw| {
                let parts: Vec<f64> = raw
                    .split_whitespace()
                    .take(3)
                    .filter_map(|p| p.parse::<f64>().ok())
                    .collect();
                (
                    *parts.first().unwrap_or(&0.0),
                    *parts.get(1).unwrap_or(&0.0),
                    *parts.get(2).unwrap_or(&0.0),
                )
            });

    let mut user_percent = 0.0;
    let mut system_percent = 0.0;
    let mut idle_percent = 100.0;
    if let Ok(stat) = std::fs::read_to_string("/proc/stat")
        && let Some(curr) = parse_proc_stat(&stat)
    {
        if let Some(prev) = collector.prev_cpu {
            let user_delta = (curr.user + curr.nice).saturating_sub(prev.user + prev.nice);
            let kernel_delta = (curr.system + curr.irq + curr.softirq)
                .saturating_sub(prev.system + prev.irq + prev.softirq);
            let idle_delta = (curr.idle + curr.iowait).saturating_sub(prev.idle + prev.iowait);
            let total_ticks = (user_delta + kernel_delta + idle_delta).max(1) as f64;
            user_percent = (user_delta as f64 / total_ticks) * 100.0;
            system_percent = (kernel_delta as f64 / total_ticks) * 100.0;
            idle_percent = (idle_delta as f64 / total_ticks) * 100.0;
        }
        collector.prev_cpu = Some(curr);
    }

    CpuTelemetry {
        brand,
        logical_cores,
        physical_cores: logical_cores,
        performance_cores: None,
        efficiency_cores: None,
        user_percent,
        system_percent,
        idle_percent,
        total_percent: (user_percent + system_percent).clamp(0.0, 100.0),
        load_avg_1m: one_min,
        load_avg_5m: five_min,
        load_avg_15m: fifteen_min,
        source: "/proc".to_owned(),
    }
}

#[cfg(target_os = "linux")]
#[allow(
    clippy::cast_precision_loss,
    reason = "byte deltas and millisecond intervals fit safely in f64 for throughput display"
)]
fn collect_linux_network(collector: &mut TelemetryCollector) -> NetworkUsageTelemetry {
    let mut interfaces =
        std::fs::read_to_string("/proc/net/dev").map_or_default(|raw| parse_proc_net_dev(&raw));
    let now = Instant::now();
    if let Some(prev) = &collector.prev_net {
        let elapsed = now.duration_since(prev.captured_at).as_secs_f64();
        if elapsed > 0.01 {
            for iface in &mut interfaces {
                if let Some(&(prev_in, prev_out)) = prev.interfaces.get(&iface.name) {
                    iface.rx_bytes_per_sec =
                        (iface.rx_bytes.saturating_sub(prev_in) as f64) / elapsed;
                    iface.tx_bytes_per_sec =
                        (iface.tx_bytes.saturating_sub(prev_out) as f64) / elapsed;
                }
            }
        }
    }
    collector.prev_net = Some(NetBaseline {
        captured_at: now,
        interfaces: interfaces
            .iter()
            .map(|iface| (iface.name.clone(), (iface.rx_bytes, iface.tx_bytes)))
            .collect(),
    });
    let received_bytes = interfaces
        .iter()
        .filter(|iface| iface.name != "lo")
        .map(|iface| iface.rx_bytes)
        .sum();
    let transmitted_bytes = interfaces
        .iter()
        .filter(|iface| iface.name != "lo")
        .map(|iface| iface.tx_bytes)
        .sum();
    let received_rate = interfaces
        .iter()
        .filter(|iface| iface.name != "lo")
        .map(|iface| iface.rx_bytes_per_sec)
        .sum();
    let transmitted_rate = interfaces
        .iter()
        .filter(|iface| iface.name != "lo")
        .map(|iface| iface.tx_bytes_per_sec)
        .sum();

    NetworkUsageTelemetry {
        interfaces,
        total_rx_bytes: received_bytes,
        total_tx_bytes: transmitted_bytes,
        total_rx_bytes_per_sec: received_rate,
        total_tx_bytes_per_sec: transmitted_rate,
        top_processes: Vec::new(),
    }
}

#[cfg(target_os = "linux")]
fn collect_platform_snapshot(
    collector: &mut TelemetryCollector,
    target_pid: Option<u32>,
    sample_window: Duration,
) -> SystemTelemetrySnapshot {
    prime_linux_baselines(collector, sample_window);
    let cpu = collect_linux_cpu(collector);
    let memory =
        std::fs::read_to_string("/proc/meminfo").map_or_default(|raw| parse_proc_meminfo(&raw));
    let network = collect_linux_network(collector);
    let volumes = run_command_stdout("df", &["-kP"]).map_or_default(|raw| parse_df_k(&raw, false));

    SystemTelemetrySnapshot {
        timestamp_ms: now_ms(),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        hostname: detect_hostname(),
        cpu,
        memory,
        gpu: GpuTelemetry {
            available: false,
            name: "No GPU telemetry driver configured on Linux".to_owned(),
            ..GpuTelemetry::default()
        },
        neural_engine: NeuralEngineTelemetry {
            available: false,
            state_label: "unavailable".to_owned(),
            note: Some("Apple Neural Engine telemetry is macOS Apple Silicon only".to_owned()),
            ..NeuralEngineTelemetry::default()
        },
        network,
        storage: StorageTelemetry {
            volumes,
            io_stats: Vec::new(),
            ssd_health: None,
        },
        target_process: target_pid.and_then(collect_unix_process),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn collect_platform_snapshot(
    _collector: &mut TelemetryCollector,
    target_pid: Option<u32>,
    _sample_window: Duration,
) -> SystemTelemetrySnapshot {
    let logical_cores = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    SystemTelemetrySnapshot {
        timestamp_ms: now_ms(),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        hostname: detect_hostname(),
        cpu: CpuTelemetry {
            brand: std::env::consts::ARCH.to_owned(),
            logical_cores,
            physical_cores: logical_cores,
            idle_percent: 100.0,
            source: "std".to_owned(),
            ..CpuTelemetry::default()
        },
        memory: MemoryTelemetry::default(),
        gpu: GpuTelemetry::default(),
        neural_engine: NeuralEngineTelemetry {
            available: false,
            state_label: "unavailable".to_owned(),
            note: Some("Apple Neural Engine telemetry is macOS Apple Silicon only".to_owned()),
            ..NeuralEngineTelemetry::default()
        },
        network: NetworkUsageTelemetry::default(),
        storage: StorageTelemetry::default(),
        target_process: target_pid.map(|pid| ProcessTelemetry {
            pid,
            name: "unknown".to_owned(),
            state: "unknown".to_owned(),
            ..ProcessTelemetry::default()
        }),
    }
}

#[cfg(target_os = "linux")]
fn parse_proc_stat(stat: &str) -> Option<LinuxCpuTicks> {
    let cpu_line = stat.lines().find(|line| line.starts_with("cpu "))?;
    let nums: Vec<u64> = cpu_line
        .split_whitespace()
        .skip(1)
        .filter_map(|v| v.parse::<u64>().ok())
        .collect();
    if nums.len() < 4 {
        return None;
    }
    Some(LinuxCpuTicks {
        user: *nums.first().unwrap_or(&0),
        nice: *nums.get(1).unwrap_or(&0),
        system: *nums.get(2).unwrap_or(&0),
        idle: *nums.get(3).unwrap_or(&0),
        iowait: *nums.get(4).unwrap_or(&0),
        irq: *nums.get(5).unwrap_or(&0),
        softirq: *nums.get(6).unwrap_or(&0),
    })
}

/// Parses Linux `/proc/meminfo` text into [`MemoryTelemetry`].
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    reason = "memory byte counts fit cleanly in f64 for pressure percentage"
)]
pub fn parse_proc_meminfo(meminfo: &str) -> MemoryTelemetry {
    let mut kv: HashMap<&str, u64> = HashMap::new();
    for line in meminfo.lines() {
        if let Some((key, rest)) = line.split_once(':')
            && let Some(kb) = rest
                .split_whitespace()
                .next()
                .and_then(|val| val.parse::<u64>().ok())
        {
            kv.insert(key.trim(), kb * 1024);
        }
    }
    let total = *kv.get("MemTotal").unwrap_or(&0);
    let free = *kv.get("MemFree").unwrap_or(&0);
    let available = *kv.get("MemAvailable").unwrap_or(&free);
    let used = total.saturating_sub(available);
    let active = *kv.get("Active").unwrap_or(&0);
    let inactive = *kv.get("Inactive").unwrap_or(&0);
    let swap_total = *kv.get("SwapTotal").unwrap_or(&0);
    let swap_free = *kv.get("SwapFree").unwrap_or(&0);
    let swap_used = swap_total.saturating_sub(swap_free);
    let pressure = if total > 0 {
        (used as f64 / total as f64) * 100.0
    } else {
        0.0
    };
    MemoryTelemetry {
        total_bytes: total,
        used_bytes: used,
        available_bytes: available,
        free_bytes: free,
        active_bytes: active,
        inactive_bytes: inactive,
        wired_bytes: *kv.get("KernelStack").unwrap_or(&0),
        compressed_bytes: 0,
        purgeable_bytes: *kv.get("Cached").unwrap_or(&0),
        swap_total_bytes: swap_total,
        swap_used_bytes: swap_used,
        swap_free_bytes: swap_free,
        page_size: 4096,
        pageins: 0,
        pageouts: 0,
        pressure_percent: pressure,
    }
}

/// Parses Linux `/proc/net/dev` text into interface telemetry entries.
#[must_use]
pub fn parse_proc_net_dev(content: &str) -> Vec<NetworkInterfaceTelemetry> {
    let mut interfaces = Vec::new();
    for line in content.lines().skip(2) {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        let nums: Vec<u64> = rest
            .split_whitespace()
            .filter_map(|v| v.parse::<u64>().ok())
            .collect();
        if nums.len() < 11 {
            continue;
        }
        interfaces.push(NetworkInterfaceTelemetry {
            name: name.trim().to_owned(),
            mtu: 1500,
            rx_bytes: nums[0],
            rx_packets: nums[1],
            rx_errors: nums[2],
            tx_bytes: nums[8],
            tx_packets: nums[9],
            tx_errors: nums[10],
            rx_bytes_per_sec: 0.0,
            tx_bytes_per_sec: 0.0,
        });
    }
    interfaces
}

/// Parses macOS `sysctl vm.loadavg` (`{ 6.10 4.66 4.07 }`).
#[must_use]
pub fn parse_sysctl_loadavg(raw: &str) -> (f64, f64, f64) {
    let nums: Vec<f64> = raw
        .trim_matches(&['{', '}', ' ', '\n'][..])
        .split_whitespace()
        .filter_map(|part| part.parse::<f64>().ok())
        .collect();
    (
        *nums.first().unwrap_or(&0.0),
        *nums.get(1).unwrap_or(&0.0),
        *nums.get(2).unwrap_or(&0.0),
    )
}

/// Parses macOS `sysctl vm.swapusage` (`total = 10240.00M  used = 8559.19M  free = 1680.81M`).
#[must_use]
pub fn parse_sysctl_swapusage(raw: &str) -> (u64, u64, u64) {
    let mut total = 0_u64;
    let mut used = 0_u64;
    let mut free = 0_u64;
    for chunk in raw.split_whitespace().collect::<Vec<_>>().windows(3) {
        if chunk[1] == "=" {
            let bytes = parse_size_token_bytes(chunk[2]);
            match chunk[0] {
                "total" => total = bytes,
                "used" => used = bytes,
                "free" => free = bytes,
                _ => {}
            }
        }
    }
    (total, used, free)
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "non-negative MiB/GiB values scale cleanly into integer byte counts"
)]
fn parse_size_token_bytes(token: &str) -> u64 {
    let trimmed = token.trim();
    let (num_str, multiplier) = if let Some(s) = trimmed.strip_suffix('G') {
        (s, 1024.0 * 1024.0 * 1024.0)
    } else if let Some(s) = trimmed.strip_suffix('M') {
        (s, 1024.0 * 1024.0)
    } else if let Some(s) = trimmed.strip_suffix('K') {
        (s, 1024.0)
    } else {
        (trimmed, 1.0)
    };
    let val = num_str.parse::<f64>().unwrap_or(0.0).max(0.0);
    (val * multiplier) as u64
}

/// Parses the last `CPU usage: X% user, Y% sys, Z% idle` line from macOS `top` output.
#[must_use]
pub fn parse_top_cpu_output(output: &str) -> Option<(f64, f64, f64)> {
    let line = output
        .lines()
        .rfind(|line| line.trim_start().starts_with("CPU usage:"))?;
    let mut user = None;
    let mut sys = None;
    let mut idle = None;
    for part in line.split(',') {
        let trimmed = part.trim();
        let num = trimmed
            .split_whitespace()
            .find_map(|tok| tok.strip_suffix('%')?.parse::<f64>().ok());
        if trimmed.contains("user") {
            user = num;
        } else if trimmed.contains("sys") {
            sys = num;
        } else if trimmed.contains("idle") {
            idle = num;
        }
    }
    Some((user?, sys?, idle?))
}

/// Parses macOS `vm_stat` output into [`MemoryTelemetry`].
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    reason = "memory byte counts fit cleanly in f64 for pressure percentage"
)]
pub fn parse_vm_stat(output: &str, total_bytes: u64) -> MemoryTelemetry {
    let mut page_size = 16384_u64;
    let mut pages_free = 0_u64;
    let mut pages_active = 0_u64;
    let mut pages_inactive = 0_u64;
    let mut pages_speculative = 0_u64;
    let mut pages_wired = 0_u64;
    let mut pages_purgeable = 0_u64;
    let mut pages_compressed = 0_u64;
    let mut pageins = 0_u64;
    let mut pageouts = 0_u64;

    for line in output.lines() {
        if let Some(rest) = line.split("page size of ").nth(1)
            && let Some(bytes_str) = rest.split_whitespace().next()
            && let Ok(parsed) = bytes_str.parse::<u64>()
        {
            page_size = parsed;
            continue;
        }
        let Some((key, val_str)) = line.split_once(':') else {
            continue;
        };
        let clean_val = val_str.trim().trim_end_matches('.');
        let Ok(value) = clean_val.parse::<u64>() else {
            continue;
        };
        match key.trim().trim_matches('"') {
            "Pages free" => pages_free = value,
            "Pages active" => pages_active = value,
            "Pages inactive" => pages_inactive = value,
            "Pages speculative" => pages_speculative = value,
            "Pages wired down" => pages_wired = value,
            "Pages purgeable" => pages_purgeable = value,
            "Pages occupied by compressor" => pages_compressed = value,
            "Pageins" => pageins = value,
            "Pageouts" => pageouts = value,
            _ => {}
        }
    }

    let free_bytes = pages_free.saturating_add(pages_speculative) * page_size;
    let active_bytes = pages_active * page_size;
    let inactive_bytes = pages_inactive * page_size;
    let wired_bytes = pages_wired * page_size;
    let compressed_bytes = pages_compressed * page_size;
    let purgeable_bytes = pages_purgeable * page_size;
    let used_bytes = active_bytes
        .saturating_add(wired_bytes)
        .saturating_add(compressed_bytes);
    let effective_total = if total_bytes > 0 {
        total_bytes
    } else {
        used_bytes
            .saturating_add(free_bytes)
            .saturating_add(inactive_bytes)
    };
    let available_bytes = effective_total.saturating_sub(used_bytes);
    let pressure_percent = if effective_total > 0 {
        ((used_bytes as f64 / effective_total as f64) * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };

    MemoryTelemetry {
        total_bytes: effective_total,
        used_bytes,
        available_bytes,
        free_bytes,
        active_bytes,
        inactive_bytes,
        wired_bytes,
        compressed_bytes,
        purgeable_bytes,
        swap_total_bytes: 0,
        swap_used_bytes: 0,
        swap_free_bytes: 0,
        page_size,
        pageins,
        pageouts,
        pressure_percent,
    }
}

/// Parses `ioreg -r -d 1 -c IOAccelerator` output into [`GpuTelemetry`].
#[must_use]
pub fn parse_ioreg_gpu(output: &str) -> GpuTelemetry {
    if output.trim().is_empty() {
        return GpuTelemetry::default();
    }
    let name = extract_ioreg_string(output, "model").unwrap_or_else(|| "Apple GPU".to_owned());
    let core_count =
        extract_ioreg_u64(output, "gpu-core-count").and_then(|val| u32::try_from(val).ok());
    let driver = extract_ioreg_string(output, "MetalPluginName");
    let perf = extract_ioreg_inline_dict(output, "PerformanceStatistics");
    let pm = extract_ioreg_inline_dict(output, "IOPowerManagement");

    let device_utilization_percent = perf
        .as_deref()
        .and_then(|d| extract_dict_f64(d, "Device Utilization %"));
    let renderer_utilization_percent = perf
        .as_deref()
        .and_then(|d| extract_dict_f64(d, "Renderer Utilization %"));
    let tiler_utilization_percent = perf
        .as_deref()
        .and_then(|d| extract_dict_f64(d, "Tiler Utilization %"));
    let in_use_memory_bytes = perf
        .as_deref()
        .and_then(|d| extract_dict_u64(d, "In use system memory"));
    let allocated_memory_bytes = perf
        .as_deref()
        .and_then(|d| extract_dict_u64(d, "Alloc system memory"));
    let parameter_buffer_bytes = perf
        .as_deref()
        .and_then(|d| extract_dict_u64(d, "Allocated PB Size"));
    let recovery_count = perf
        .as_deref()
        .and_then(|d| extract_dict_u64(d, "recoveryCount"));
    let power_state = pm
        .as_deref()
        .and_then(|d| extract_dict_u64(d, "CurrentPowerState"))
        .map(|state| if state > 0 { "active" } else { "idle" }.to_owned());

    GpuTelemetry {
        available: true,
        name,
        core_count,
        driver,
        device_utilization_percent,
        renderer_utilization_percent,
        tiler_utilization_percent,
        in_use_memory_bytes,
        allocated_memory_bytes,
        parameter_buffer_bytes,
        recovery_count,
        power_state,
    }
}

/// Parses `ioreg` outputs from `H11ANEIn` and `H1xANELoadBalancer` into [`NeuralEngineTelemetry`].
#[must_use]
pub fn parse_ioreg_ane(h11_output: &str, lb_output: &str) -> NeuralEngineTelemetry {
    if h11_output.trim().is_empty() && lb_output.trim().is_empty() {
        return NeuralEngineTelemetry {
            available: false,
            state_label: "unavailable".to_owned(),
            ..NeuralEngineTelemetry::default()
        };
    }

    let props = extract_ioreg_inline_dict(h11_output, "DeviceProperties");
    let lb_props = extract_ioreg_inline_dict(lb_output, "DeviceProperties");
    let pm = extract_ioreg_inline_dict(h11_output, "IOPowerManagement");

    let architecture = props
        .as_deref()
        .and_then(|d| extract_dict_string(d, "ANEDevicePropertyTypeANEArchitectureTypeStr"));
    let core_count = props
        .as_deref()
        .and_then(|d| extract_dict_u64(d, "ANEDevicePropertyNumANECores"))
        .and_then(|val| u32::try_from(val).ok());
    let unit_count = lb_props
        .as_deref()
        .and_then(|d| extract_dict_u64(d, "ANEDevicePropertyNumANEs"))
        .and_then(|val| u32::try_from(val).ok());
    let power_state = pm
        .as_deref()
        .and_then(|d| extract_dict_u64(d, "CurrentPowerState"))
        .and_then(|val| u32::try_from(val).ok());
    let max_power_state = pm
        .as_deref()
        .and_then(|d| extract_dict_u64(d, "MaxPowerState"))
        .and_then(|val| u32::try_from(val).ok());

    let state_label = match power_state {
        Some(state) if state > 0 => "active".to_owned(),
        Some(_) => "idle".to_owned(),
        None => "ready".to_owned(),
    };

    NeuralEngineTelemetry {
        available: true,
        architecture,
        core_count,
        unit_count,
        power_state,
        max_power_state,
        state_label,
        power_mw: None,
        active_clients: Vec::new(),
        note: None,
    }
}

/// Parses `netstat -ibn` link-layer (`<Link#...>`) rows into interface counters.
#[must_use]
pub fn parse_netstat_ibn(output: &str) -> Vec<NetworkInterfaceTelemetry> {
    let mut by_name: BTreeMap<String, NetworkInterfaceTelemetry> = BTreeMap::new();
    for line in output.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 10 {
            continue;
        }
        // Only link-level rows report interface-wide counters accurately without duplicating IPv4/IPv6 rows.
        if !cols.iter().any(|col| col.starts_with("<Link#")) {
            continue;
        }
        let name = cols[0].trim_end_matches('*').to_owned();
        let mtu = cols[1].parse::<u64>().unwrap_or(0);
        // Columns vary depending on whether a MAC address column is present (e.g., lo0 vs en0).
        // With MAC: Name Mtu <Link#> MAC Ipkts Ierrs Ibytes Opkts Oerrs Obytes [Coll]
        // Without MAC: Name Mtu <Link#> Ipkts Ierrs Ibytes Opkts Oerrs Obytes [Coll]
        let link_idx = cols
            .iter()
            .position(|col| col.starts_with("<Link#"))
            .unwrap_or(2);
        let numeric_start = if cols.get(link_idx + 1).is_some_and(|tok| tok.contains(':')) {
            link_idx + 2
        } else {
            link_idx + 1
        };
        if cols.len() < numeric_start + 6 {
            continue;
        }
        let rx_packets = cols[numeric_start].parse::<u64>().unwrap_or(0);
        let rx_errors = cols[numeric_start + 1].parse::<u64>().unwrap_or(0);
        let rx_bytes = cols[numeric_start + 2].parse::<u64>().unwrap_or(0);
        let tx_packets = cols[numeric_start + 3].parse::<u64>().unwrap_or(0);
        let tx_errors = cols[numeric_start + 4].parse::<u64>().unwrap_or(0);
        let tx_bytes = cols[numeric_start + 5].parse::<u64>().unwrap_or(0);

        by_name.insert(
            name.clone(),
            NetworkInterfaceTelemetry {
                name,
                mtu,
                rx_bytes,
                tx_bytes,
                rx_packets,
                tx_packets,
                rx_errors,
                tx_errors,
                rx_bytes_per_sec: 0.0,
                tx_bytes_per_sec: 0.0,
            },
        );
    }
    by_name.into_values().collect()
}

/// Parses `nettop -P -L 1 -J bytes_in,bytes_out` CSV lines (`name.pid,bytes_in,bytes_out,`).
#[must_use]
pub fn parse_nettop_csv(output: &str) -> Vec<ProcessNetworkUsage> {
    let mut items = Vec::new();
    for line in output.lines().skip(1) {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() < 3 {
            continue;
        }
        let proc_pid = cols[0].trim();
        let Some((name, pid_str)) = proc_pid.rsplit_once('.') else {
            continue;
        };
        let Ok(pid) = pid_str.parse::<u32>() else {
            continue;
        };
        let rx_bytes = cols[1].trim().parse::<u64>().unwrap_or(0);
        let tx_bytes = cols[2].trim().parse::<u64>().unwrap_or(0);
        items.push(ProcessNetworkUsage {
            pid,
            name: name.to_owned(),
            rx_bytes,
            tx_bytes,
            rx_bytes_per_sec: 0.0,
            tx_bytes_per_sec: 0.0,
        });
    }
    items
}

/// Parses POSIX `df -k` output into [`StorageVolumeTelemetry`] records.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    reason = "filesystem byte capacities fit cleanly in f64 for usage percentages"
)]
pub fn parse_df_k(output: &str, filter_virtual: bool) -> Vec<StorageVolumeTelemetry> {
    let mut volumes = Vec::new();
    for line in output.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 6 {
            continue;
        }
        let filesystem = cols[0];
        if filter_virtual
            && (filesystem == "devfs"
                || filesystem.starts_with("map ")
                || cols[cols.len() - 1].starts_with("/System/Volumes/Preboot")
                || cols[cols.len() - 1].starts_with("/System/Volumes/Update")
                || cols[cols.len() - 1].starts_with("/System/Volumes/xarts")
                || cols[cols.len() - 1].starts_with("/System/Volumes/iSCPreboot")
                || cols[cols.len() - 1].starts_with("/System/Volumes/Hardware"))
        {
            continue;
        }
        let Ok(total_kb) = cols[1].parse::<u64>() else {
            continue;
        };
        let used_kb = cols[2].parse::<u64>().unwrap_or(0);
        let avail_kb = cols[3].parse::<u64>().unwrap_or(0);
        let mount_point = cols[cols.len() - 1].to_owned();
        let total_bytes = total_kb.saturating_mul(1024);
        let used_bytes = used_kb.saturating_mul(1024);
        let available_bytes = avail_kb.saturating_mul(1024);
        let usage_percent = if total_bytes > 0 {
            ((used_bytes as f64) / (total_bytes as f64) * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };
        volumes.push(StorageVolumeTelemetry {
            filesystem: filesystem.to_owned(),
            mount_point,
            total_bytes,
            used_bytes,
            available_bytes,
            usage_percent,
        });
    }
    volumes
}

/// Parses `iostat -d -c 1` output into instantaneous [`DiskIoTelemetry`] records.
#[must_use]
pub fn parse_iostat_output(output: &str) -> Vec<DiskIoTelemetry> {
    let lines: Vec<&str> = output.lines().collect();
    if lines.len() < 3 {
        return Vec::new();
    }
    let devices: Vec<&str> = lines[0]
        .split_whitespace()
        .take_while(|&name| name != "cpu")
        .collect();
    let values: Vec<f64> = lines[lines.len() - 1]
        .split_whitespace()
        .filter_map(|v| v.parse::<f64>().ok())
        .collect();
    let mut stats = Vec::new();
    for (idx, device) in devices.iter().enumerate() {
        let base = idx * 3;
        if values.len() < base + 3 {
            break;
        }
        stats.push(DiskIoTelemetry {
            device: (*device).to_owned(),
            read_mb_per_sec: values[base + 2],
            write_mb_per_sec: 0.0,
            iops: values[base + 1],
        });
    }
    stats
}

/// Parses cumulative read/write bytes and operations from `ioreg -r -c IOBlockStorageDriver -k Statistics`.
#[must_use]
pub fn parse_ioreg_block_storage_stats(output: &str) -> Option<(u64, u64, u64, u64)> {
    let mut read_bytes_sum = 0_u64;
    let mut write_bytes_sum = 0_u64;
    let mut read_ops_sum = 0_u64;
    let mut write_ops_sum = 0_u64;
    let mut found = false;

    for line in output.lines() {
        if let Some(dict) = extract_ioreg_inline_dict(line, "Statistics") {
            found = true;
            read_bytes_sum =
                read_bytes_sum.saturating_add(extract_dict_u64(&dict, "Bytes (Read)").unwrap_or(0));
            write_bytes_sum = write_bytes_sum
                .saturating_add(extract_dict_u64(&dict, "Bytes (Write)").unwrap_or(0));
            read_ops_sum = read_ops_sum
                .saturating_add(extract_dict_u64(&dict, "Operations (Read)").unwrap_or(0));
            write_ops_sum = write_ops_sum
                .saturating_add(extract_dict_u64(&dict, "Operations (Write)").unwrap_or(0));
        }
    }
    found.then_some((read_bytes_sum, write_bytes_sum, read_ops_sum, write_ops_sum))
}

/// Parses macOS `diskutil info -plist` output for `NVMe` SSD SMART health & wear counters.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    reason = "SMART Kelvin temperature values are small integers (<1000)"
)]
pub fn parse_diskutil_smart_plist(plist: &str) -> SsdHealthTelemetry {
    let device_name = extract_plist_string_value(plist, "MediaName")
        .or_else(|| extract_plist_string_value(plist, "VolumeName"))
        .or_else(|| extract_plist_string_value(plist, "IORegistryEntryName"));
    let protocol = extract_plist_string_value(plist, "BusProtocol");
    let smart_status = extract_plist_string_value(plist, "SMARTStatus");
    let solid_state = extract_plist_bool(plist, "SolidState").unwrap_or(false);

    let temperature_celsius = extract_plist_integer(plist, "TEMPERATURE").map(|raw| {
        if raw > 200 {
            (raw as f64) - 273.15
        } else {
            raw as f64
        }
    });
    let percentage_used = extract_plist_integer(plist, "PERCENTAGE_USED");
    let available_spare_percent = extract_plist_integer(plist, "AVAILABLE_SPARE");
    let bytes_read_lifetime = extract_plist_integer(plist, "DATA_UNITS_READ_0")
        .map(|units| units.saturating_mul(512_000));
    let bytes_written_lifetime = extract_plist_integer(plist, "DATA_UNITS_WRITTEN_0")
        .map(|units| units.saturating_mul(512_000));
    let power_on_hours = extract_plist_integer(plist, "POWER_ON_HOURS_0");
    let power_cycles = extract_plist_integer(plist, "POWER_CYCLES_0");
    let media_errors = extract_plist_integer(plist, "MEDIA_ERRORS_0");
    let unsafe_shutdowns = extract_plist_integer(plist, "UNSAFE_SHUTDOWNS_0");

    SsdHealthTelemetry {
        device_name,
        protocol,
        smart_status,
        solid_state,
        temperature_celsius,
        percentage_used,
        available_spare_percent,
        bytes_read_lifetime,
        bytes_written_lifetime,
        power_on_hours,
        power_cycles,
        media_errors,
        unsafe_shutdowns,
    }
}

/// Parses one `ps -Ao pid=,pcpu=,rss=,comm=` line.
#[must_use]
pub fn parse_ps_line(line: &str) -> Option<ProcessSample> {
    let cols: Vec<&str> = line.split_whitespace().collect();
    if cols.len() < 4 {
        return None;
    }
    let pid = cols[0].parse::<u32>().ok()?;
    let cpu_percent = cols[1].replace(',', ".").parse::<f64>().unwrap_or(0.0);
    let rss_kb = cols[2].parse::<u64>().unwrap_or(0);
    let name = cols[3..].join(" ");
    Some(ProcessSample {
        pid,
        name,
        cpu_percent,
        memory_rss_bytes: rss_kb * 1024,
    })
}

fn extract_ioreg_string(source: &str, key: &str) -> Option<String> {
    let marker = format!("\"{key}\" = \"");
    let rest = source.split(&marker).nth(1)?;
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

fn extract_ioreg_u64(source: &str, key: &str) -> Option<u64> {
    let marker = format!("\"{key}\" = ");
    let rest = source.split(&marker).nth(1)?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<u64>().ok()
}

fn extract_ioreg_inline_dict(source: &str, key: &str) -> Option<String> {
    let marker = format!("\"{key}\" = {{");
    let rest = source.split(&marker).nth(1)?;
    let end = rest.find('}')?;
    Some(rest[..end].to_owned())
}

fn extract_dict_string(dict: &str, key: &str) -> Option<String> {
    let marker = format!("\"{key}\"=\"");
    let rest = dict.split(&marker).nth(1)?;
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

fn extract_dict_u64(dict: &str, key: &str) -> Option<u64> {
    let marker = format!("\"{key}\"=");
    let rest = dict.split(&marker).nth(1)?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<u64>().ok()
}

fn extract_dict_f64(dict: &str, key: &str) -> Option<f64> {
    let marker = format!("\"{key}\"=");
    let rest = dict.split(&marker).nth(1)?;
    let token: String = rest
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect();
    token.parse::<f64>().ok()
}

fn extract_plist_string_value(plist: &str, key: &str) -> Option<String> {
    let marker = format!("<key>{key}</key>");
    let rest = plist.split(&marker).nth(1)?;
    let start = rest.find("<string>")? + "<string>".len();
    let after = &rest[start..];
    let end = after.find("</string>")?;
    let trimmed = after[..end].trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn extract_plist_integer(plist: &str, key: &str) -> Option<u64> {
    let marker = format!("<key>{key}</key>");
    let rest = plist.split(&marker).nth(1)?;
    let start = rest.find("<integer>")? + "<integer>".len();
    let after = &rest[start..];
    let end = after.find("</integer>")?;
    after[..end].trim().parse::<u64>().ok()
}

fn extract_plist_bool(plist: &str, key: &str) -> Option<bool> {
    let marker = format!("<key>{key}</key>");
    let rest = plist.split(&marker).nth(1)?;
    let trimmed = rest.trim_start();
    if trimmed.starts_with("<true/>") {
        Some(true)
    } else if trimmed.starts_with("<false/>") {
        Some(false)
    } else {
        None
    }
}

fn run_command_stdout(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        None
    }
}

fn detect_hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok()
        .or_else(|| run_command_stdout("hostname", &[]).map(|value| value.trim().to_owned()))
        .unwrap_or_else(|| "localhost".to_owned())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::{
        parse_df_k, parse_diskutil_smart_plist, parse_ioreg_ane, parse_ioreg_block_storage_stats,
        parse_ioreg_gpu, parse_iostat_output, parse_netstat_ibn, parse_nettop_csv,
        parse_proc_meminfo, parse_proc_net_dev, parse_sysctl_swapusage, parse_top_cpu_output,
        parse_vm_stat,
    };

    #[test]
    fn parses_macos_vm_stat_and_swap() {
        let vm = "\
Mach Virtual Memory Statistics: (page size of 16384 bytes)
Pages free:                                    76718.
Pages active:                                 645468.
Pages inactive:                               642323.
Pages speculative:                              1929.
Pages wired down:                             395920.
Pages purgeable:                               19326.
Pages occupied by compressor:                 541602.
Pageins:                                   179643744.
Pageouts:                                      40092.
";
        let mem = parse_vm_stat(vm, 38_654_705_664);
        assert_eq!(mem.page_size, 16384);
        assert_eq!(mem.pageins, 179_643_744);
        assert!(mem.used_bytes > 20_000_000_000);
        assert!(mem.pressure_percent > 50.0);

        let (total, used, free) = parse_sysctl_swapusage(
            "total = 10240.00M  used = 8559.19M  free = 1680.81M  (encrypted)",
        );
        assert_eq!(total, 10_240 * 1024 * 1024);
        assert!(used > free);
    }

    #[test]
    fn parses_top_cpu_and_ioreg_gpu_and_ane() {
        let top = "\
CPU usage: 10.00% user, 5.00% sys, 85.00% idle
CPU usage: 11.91% user, 13.80% sys, 74.28% idle
";
        let (u, s, i) = parse_top_cpu_output(top).expect("cpu line");
        assert!((u - 11.91).abs() < 0.01);
        assert!((s - 13.80).abs() < 0.01);
        assert!((i - 74.28).abs() < 0.01);

        let gpu_ioreg = r#"
      "model" = "Apple M3 Max"
      "gpu-core-count" = 30
      "MetalPluginName" = "AGXMetalG15X_M1"
      "IOPowerManagement" = {"CurrentPowerState"=1,"MaxPowerState"=1}
      "PerformanceStatistics" = {"Alloc system memory"=6386008064,"Tiler Utilization %"=23,"Renderer Utilization %"=24,"Device Utilization %"=25,"Allocated PB Size"=212336640,"In use system memory"=3204317184,"recoveryCount"=0}
"#;
        let gpu = parse_ioreg_gpu(gpu_ioreg);
        assert!(gpu.available);
        assert_eq!(gpu.name, "Apple M3 Max");
        assert_eq!(gpu.core_count, Some(30));
        assert_eq!(gpu.device_utilization_percent, Some(25.0));
        assert_eq!(gpu.renderer_utilization_percent, Some(24.0));
        assert_eq!(gpu.tiler_utilization_percent, Some(23.0));
        assert_eq!(gpu.in_use_memory_bytes, Some(3_204_317_184));

        let ane_h11 = r#"
      "IOPowerManagement" = {"DevicePowerState"=0,"CurrentPowerState"=0,"MaxPowerState"=1}
      "DeviceProperties" = {"ANEDevicePropertyNumANECores"=16,"ANEDevicePropertyTypeANEArchitectureTypeStr"="h15g"}
"#;
        let ane_lb = r#"
      "DeviceProperties" = {"ANEDevicePropertyNumANEs"=1}
"#;
        let ane = parse_ioreg_ane(ane_h11, ane_lb);
        assert!(ane.available);
        assert_eq!(ane.core_count, Some(16));
        assert_eq!(ane.unit_count, Some(1));
        assert_eq!(ane.architecture.as_deref(), Some("h15g"));
        assert_eq!(ane.state_label, "idle");
    }

    #[test]
    fn parses_network_and_storage_telemetry() {
        let netstat = "\
Name       Mtu   Network       Address            Ipkts Ierrs     Ibytes    Opkts Oerrs     Obytes  Coll
lo0        16384 <Link#1>                      23366917     0 39209946192 23366917     0 39209946192     0
en0        1500  <Link#12>   aa:bb:cc:dd:ee:ff    1000     1   5000000    800     0   2000000     0
en0        1500  192.168.1     192.168.1.10       1000     -   5000000    800     -   2000000     -
";
        let ifaces = parse_netstat_ibn(netstat);
        assert_eq!(ifaces.len(), 2);
        assert_eq!(ifaces[0].name, "en0");
        assert_eq!(ifaces[0].rx_bytes, 5_000_000);
        assert_eq!(ifaces[0].tx_bytes, 2_000_000);

        let nettop = ",bytes_in,bytes_out,\nferro.42,1024,2048,\n";
        let procs = parse_nettop_csv(nettop);
        assert_eq!(procs.len(), 1);
        assert_eq!(procs[0].pid, 42);
        assert_eq!(procs[0].tx_bytes, 2048);

        let df = "\
Filesystem     1024-blocks      Used Available Capacity iused ifree %iused Mounted on
/dev/disk3s1s1   971350180  13437184   8963104    60%  486811 89631040    1%   /
";
        let vols = parse_df_k(df, true);
        assert_eq!(vols.len(), 1);
        assert_eq!(vols[0].mount_point, "/");

        let plist = "\
<dict>
  <key>MediaName</key><string>AppleAPFSMedia</string>
  <key>BusProtocol</key><string>Apple Fabric</string>
  <key>SMARTStatus</key><string>Verified</string>
  <key>SolidState</key><true/>
  <key>PERCENTAGE_USED</key><integer>9</integer>
  <key>AVAILABLE_SPARE</key><integer>100</integer>
  <key>DATA_UNITS_READ_0</key><integer>1000</integer>
  <key>DATA_UNITS_WRITTEN_0</key><integer>2000</integer>
  <key>TEMPERATURE</key><integer>311</integer>
</dict>
";
        let smart = parse_diskutil_smart_plist(plist);
        assert!(smart.solid_state);
        assert_eq!(smart.percentage_used, Some(9));
        assert_eq!(smart.bytes_read_lifetime, Some(512_000_000));
        assert!((smart.temperature_celsius.expect("temp") - 37.85).abs() < 0.1);

        let iostat = "\
              disk0       cpu
    KB/t  tps  MB/s  us sy id
   19.53  592 11.29  12  6 82
";
        let io = parse_iostat_output(iostat);
        assert_eq!(io.len(), 1);
        assert!((io[0].read_mb_per_sec - 11.29).abs() < 0.01);

        let ioreg_block = r#""Statistics" = {"Bytes (Read)"=1048576,"Bytes (Write)"=2097152,"Operations (Read)"=10,"Operations (Write)"=20}"#;
        assert_eq!(
            parse_ioreg_block_storage_stats(ioreg_block),
            Some((1_048_576, 2_097_152, 10, 20))
        );
    }

    #[test]
    fn parses_linux_proc_formats() {
        let meminfo = "MemTotal:       16000000 kB\nMemFree:         2000000 kB\nMemAvailable:    8000000 kB\n";
        let mem = parse_proc_meminfo(meminfo);
        assert_eq!(mem.total_bytes, 16_000_000 * 1024);
        assert!((mem.pressure_percent - 50.0).abs() < 0.1);

        let net_dev = "\
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
  eth0: 12345 100 1 0 0 0 0 0 67890 200 2 0 0 0 0 0
";
        let ifaces = parse_proc_net_dev(net_dev);
        assert_eq!(ifaces.len(), 1);
        assert_eq!(ifaces[0].rx_bytes, 12345);
        assert_eq!(ifaces[0].tx_bytes, 67890);
    }
}
