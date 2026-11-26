//! Network inspection tools: active sockets, live interface/process traffic, endpoint probes,
//! and live `DevTools` transaction / HAR inspection.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use clap::Subcommand;
use ferro_devtools::{
    NetworkSummary, NetworkTransaction, TelemetryCollector, inspect_sockets, probe_endpoint,
};
use serde::Deserialize;

use crate::http_client::{http_get_text, resolve_devtools_url};
use crate::monitor::format_bytes;

#[derive(Debug, Subcommand)]
pub(crate) enum NetworkCommand {
    /// List active TCP/UDP sockets and listening ports.
    Connections {
        /// Filter sockets by process ID.
        #[arg(long)]
        pid: Option<u32>,
        /// Filter sockets by process name substring.
        #[arg(long)]
        process: Option<String>,
        /// Only display `LISTEN` sockets.
        #[arg(long)]
        listening: bool,
        /// Emit JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Sample per-interface and per-process network bandwidth rates.
    Traffic {
        /// Sampling window in milliseconds.
        #[arg(long, default_value_t = 800)]
        duration_ms: u64,
        /// Optional target PID.
        #[arg(long)]
        pid: Option<u32>,
        /// Emit JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Probe a live `http://`, `https://`, or `tcp://` endpoint for DNS/TCP/TLS/TTFB timings.
    Probe {
        /// Target URL to probe.
        url: String,
        /// Timeout in milliseconds.
        #[arg(long, default_value_t = 5000)]
        timeout_ms: u64,
        /// Emit JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Inspect recorded HTTP/IPC transactions from a running Ferro `DevTools` endpoint.
    Inspect {
        /// Explicit `DevTools` base URL.
        #[arg(long)]
        url: Option<String>,
        /// Running Ferro process PID.
        #[arg(long)]
        pid: Option<u32>,
        /// Filter transactions by host substring.
        #[arg(long)]
        host: Option<String>,
        /// Show only failed or 4xx/5xx transactions.
        #[arg(long)]
        errors_only: bool,
        /// Export a HAR 1.2 file to disk.
        #[arg(long)]
        har: Option<PathBuf>,
        /// Emit JSON output.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Deserialize)]
struct LiveNetworkPayload {
    summary: NetworkSummary,
    transactions: Vec<NetworkTransaction>,
}

pub(crate) fn run(command: NetworkCommand) -> Result<(), String> {
    match command {
        NetworkCommand::Connections {
            pid,
            process,
            listening,
            json,
        } => run_connections(pid, process.as_deref(), listening, json),
        NetworkCommand::Traffic {
            duration_ms,
            pid,
            json,
        } => run_traffic(duration_ms, pid, json),
        NetworkCommand::Probe {
            url,
            timeout_ms,
            json,
        } => run_probe(&url, timeout_ms, json),
        NetworkCommand::Inspect {
            url,
            pid,
            host,
            errors_only,
            har,
            json,
        } => {
            inspect_live_transactions(url.as_deref(), pid, host.as_deref(), errors_only, har, json)
        }
    }
}

fn run_connections(
    pid: Option<u32>,
    process: Option<&str>,
    listening: bool,
    json: bool,
) -> Result<(), String> {
    let sockets = inspect_sockets(pid, process, listening)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&sockets).map_err(|error| error.to_string())?
        );
    } else if sockets.is_empty() {
        println!("No matching network sockets found.");
    } else {
        println!(
            "{:<18} {:<7} {:<5} {:<12} ENDPOINT",
            "PROCESS", "PID", "PROTO", "STATE"
        );
        for sock in sockets {
            println!(
                "{:<18} {:<7} {:<5} {:<12} {}",
                sock.process,
                sock.pid,
                sock.protocol,
                sock.state.as_deref().unwrap_or("-"),
                sock.endpoint
            );
        }
    }
    Ok(())
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "non-negative throughput rates fit cleanly in u64 for byte formatting"
)]
fn run_traffic(duration_ms: u64, pid: Option<u32>, json: bool) -> Result<(), String> {
    let mut collector = TelemetryCollector::new();
    let snapshot = collector.collect(pid, Duration::from_millis(duration_ms));
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&snapshot.network).map_err(|error| error.to_string())?
        );
        return Ok(());
    }
    let rx_rate = snapshot.network.total_rx_bytes_per_sec.max(0.0) as u64;
    let tx_rate = snapshot.network.total_tx_bytes_per_sec.max(0.0) as u64;
    println!(
        "Network Traffic ({duration_ms}ms window) — RX {}/s | TX {}/s",
        format_bytes(rx_rate),
        format_bytes(tx_rate)
    );
    for iface in &snapshot.network.interfaces {
        if iface.rx_bytes == 0 && iface.tx_bytes == 0 {
            continue;
        }
        let irx = iface.rx_bytes_per_sec.max(0.0) as u64;
        let itx = iface.tx_bytes_per_sec.max(0.0) as u64;
        println!(
            "  {:<8} RX {}/s (total {})  TX {}/s (total {})",
            iface.name,
            format_bytes(irx),
            format_bytes(iface.rx_bytes),
            format_bytes(itx),
            format_bytes(iface.tx_bytes)
        );
    }
    for proc_net in &snapshot.network.top_processes {
        println!(
            "  proc {:<18} (pid {:<6}) RX {}  TX {}",
            proc_net.name,
            proc_net.pid,
            format_bytes(proc_net.rx_bytes),
            format_bytes(proc_net.tx_bytes)
        );
    }
    Ok(())
}

fn run_probe(url: &str, timeout_ms: u64, json: bool) -> Result<(), String> {
    let result = probe_endpoint(url, Duration::from_millis(timeout_ms))?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&result).map_err(|error| error.to_string())?
        );
    } else {
        println!(
            "Probe {} -> status {:?} ({} bytes)",
            result.url, result.status, result.response_bytes
        );
        println!(
            "  Timings: DNS {:.2} ms | TCP {:.2} ms | TLS {:.2} ms | TTFB {:.2} ms | Total {:.2} ms",
            result.timing.dns_ms,
            result.timing.connect_ms,
            result.timing.tls_ms,
            result.timing.ttfb_ms,
            result.timing.total_ms
        );
        for header in &result.headers {
            println!("  {}: {}", header.name, header.value);
        }
    }
    Ok(())
}

fn inspect_live_transactions(
    url: Option<&str>,
    pid: Option<u32>,
    host: Option<&str>,
    errors_only: bool,
    har: Option<PathBuf>,
    json: bool,
) -> Result<(), String> {
    let base = resolve_devtools_url(url, pid)?;
    if let Some(har_path) = har {
        let har_body = http_get_text(&base, "/network/har")?;
        fs::write(&har_path, har_body)
            .map_err(|error| format!("could not write HAR `{}`: {error}", har_path.display()))?;
        println!("Exported HAR 1.2 to {}", har_path.display());
    }
    let raw = http_get_text(&base, "/network")?;
    let mut payload: LiveNetworkPayload =
        serde_json::from_str(&raw).map_err(|error| format!("invalid network payload: {error}"))?;
    if let Some(host_filter) = host {
        let needle = host_filter.to_ascii_lowercase();
        payload
            .transactions
            .retain(|tx| tx.host.to_ascii_lowercase().contains(&needle));
    }
    if errors_only {
        payload
            .transactions
            .retain(|tx| tx.error.is_some() || tx.status.is_some_and(|code| code >= 400));
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&payload.transactions)
                .map_err(|error| error.to_string())?
        );
    } else {
        println!(
            "Network Inspector — {} total ({} failed), avg {:.1} ms, p95 {:.1} ms",
            payload.summary.total_requests,
            payload.summary.failed_requests,
            payload.summary.avg_latency_ms,
            payload.summary.p95_latency_ms
        );
        for tx in payload.transactions {
            println!(
                "  #{:<4} {:<6} {:<4} {:>7.1} ms  {}  (req {} B / resp {} B)",
                tx.id,
                tx.method,
                tx.status
                    .map_or_else(|| "ERR".to_owned(), |c| c.to_string()),
                tx.timing.total_ms,
                tx.url,
                tx.request_bytes,
                tx.response_bytes
            );
        }
    }
    Ok(())
}
