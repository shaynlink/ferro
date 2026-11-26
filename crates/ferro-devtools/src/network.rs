//! HTTP/IPC network transaction inspector, sensitive-header redaction, HAR 1.2 export, socket
//! inspection, and live endpoint latency probing.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// HTTP or transport method label recorded by [`NetworkInspector`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NetworkHeader {
    /// Header name.
    pub name: String,
    /// Header value (sensitive credentials are redacted automatically).
    pub value: String,
}

/// Phase-by-phase timing breakdown for a recorded network operation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NetworkTiming {
    /// DNS lookup latency in milliseconds.
    pub dns_ms: f64,
    /// TCP connection handshake latency in milliseconds.
    pub connect_ms: f64,
    /// TLS handshake latency in milliseconds (0 for plaintext).
    pub tls_ms: f64,
    /// Time to first byte in milliseconds.
    pub ttfb_ms: f64,
    /// Total end-to-end duration in milliseconds.
    pub total_ms: f64,
}

/// Recorded network request/response transaction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetworkTransaction {
    /// Sequential identifier.
    pub id: u64,
    /// Start time in Unix epoch milliseconds.
    pub started_at_ms: u64,
    /// Request method (`GET`, `POST`, `WS`, `IPC`, etc.).
    pub method: String,
    /// Full URL or target endpoint.
    pub url: String,
    /// Extracted host authority.
    pub host: String,
    /// Response status code if received (`None` for transport failures).
    pub status: Option<u16>,
    /// Sanitized request headers.
    pub request_headers: Vec<NetworkHeader>,
    /// Sanitized response headers.
    pub response_headers: Vec<NetworkHeader>,
    /// Request payload byte count.
    pub request_bytes: u64,
    /// Response payload byte count.
    pub response_bytes: u64,
    /// Timing breakdown.
    pub timing: NetworkTiming,
    /// Optional failure description.
    pub error: Option<String>,
}

/// Query filter for searching [`NetworkInspector`] transactions.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NetworkFilter {
    /// Substring filter matched against [`NetworkTransaction::host`].
    pub host: Option<String>,
    /// Exact case-insensitive method match.
    pub method: Option<String>,
    /// Minimum HTTP status code filter (inclusive).
    pub min_status: Option<u16>,
    /// Maximum HTTP status code filter (inclusive).
    pub max_status: Option<u16>,
    /// Restricts results to failed or 4xx/5xx transactions.
    pub errors_only: bool,
    /// Minimum total latency in milliseconds.
    pub min_duration_ms: Option<u64>,
}

/// Aggregate network metrics computed over retained transactions.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NetworkSummary {
    /// Total recorded transactions.
    pub total_requests: usize,
    /// Count of requests with transport errors or status >= 400.
    pub failed_requests: usize,
    /// Total transmitted request bytes.
    pub total_request_bytes: u64,
    /// Total received response bytes.
    pub total_response_bytes: u64,
    /// Mean total latency in milliseconds.
    pub avg_latency_ms: f64,
    /// 95th percentile latency in milliseconds.
    pub p95_latency_ms: f64,
}

/// Bounded in-memory recorder for application network transactions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkInspector {
    max_entries: usize,
    next_id: u64,
    transactions: VecDeque<NetworkTransaction>,
}

impl NetworkInspector {
    /// Creates an inspector retaining up to `max_entries` transactions.
    #[must_use]
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries: max_entries.max(1),
            next_id: 1,
            transactions: VecDeque::new(),
        }
    }

    /// Records a transaction after redacting sensitive request/response headers.
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &mut self,
        method: impl Into<String>,
        url: impl Into<String>,
        status: Option<u16>,
        request_headers: Vec<NetworkHeader>,
        response_headers: Vec<NetworkHeader>,
        request_bytes: u64,
        response_bytes: u64,
        timing: NetworkTiming,
        error: Option<String>,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let url = url.into();
        let host = extract_host(&url);
        let tx = NetworkTransaction {
            id,
            started_at_ms: now_ms(),
            method: method.into().to_ascii_uppercase(),
            url,
            host,
            status,
            request_headers: sanitize_headers(request_headers),
            response_headers: sanitize_headers(response_headers),
            request_bytes,
            response_bytes,
            timing,
            error,
        };
        if self.transactions.len() == self.max_entries {
            self.transactions.pop_front();
        }
        self.transactions.push_back(tx);
        id
    }

    /// Queries retained transactions matching `filter`.
    #[must_use]
    pub fn query(&self, filter: &NetworkFilter) -> Vec<NetworkTransaction> {
        self.transactions
            .iter()
            .filter(|tx| matches_filter(tx, filter))
            .cloned()
            .collect()
    }

    /// Computes summary metrics across all retained transactions.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "transaction counts fit cleanly in f64 for average latency diagnostics"
    )]
    pub fn summary(&self) -> NetworkSummary {
        let total_requests = self.transactions.len();
        if total_requests == 0 {
            return NetworkSummary::default();
        }
        let failed_requests = self
            .transactions
            .iter()
            .filter(|tx| tx.error.is_some() || tx.status.is_some_and(|code| code >= 400))
            .count();
        let total_request_bytes = self.transactions.iter().map(|tx| tx.request_bytes).sum();
        let total_response_bytes = self.transactions.iter().map(|tx| tx.response_bytes).sum();
        let mut latencies: Vec<f64> = self
            .transactions
            .iter()
            .map(|tx| tx.timing.total_ms)
            .collect();
        latencies.sort_by(f64::total_cmp);
        let avg_latency_ms = latencies.iter().sum::<f64>() / (total_requests as f64);
        let p95_idx = ((total_requests.saturating_sub(1)) * 95) / 100;
        let p95_latency_ms = *latencies.get(p95_idx).unwrap_or(&0.0);

        NetworkSummary {
            total_requests,
            failed_requests,
            total_request_bytes,
            total_response_bytes,
            avg_latency_ms,
            p95_latency_ms,
        }
    }

    /// Exports retained transactions as a HAR 1.2 JSON document.
    #[must_use]
    pub fn to_har_json(&self) -> String {
        let entries: Vec<serde_json::Value> = self
            .transactions
            .iter()
            .map(|tx| {
                serde_json::json!({
                    "startedDateTime": format!("{}", tx.started_at_ms),
                    "time": tx.timing.total_ms,
                    "request": {
                        "method": tx.method,
                        "url": tx.url,
                        "httpVersion": "HTTP/1.1",
                        "headers": tx.request_headers,
                        "headersSize": -1,
                        "bodySize": tx.request_bytes,
                    },
                    "response": {
                        "status": tx.status.unwrap_or(0),
                        "statusText": tx.error.as_deref().unwrap_or(""),
                        "httpVersion": "HTTP/1.1",
                        "headers": tx.response_headers,
                        "headersSize": -1,
                        "bodySize": tx.response_bytes,
                    },
                    "timings": {
                        "dns": tx.timing.dns_ms,
                        "connect": tx.timing.connect_ms,
                        "ssl": tx.timing.tls_ms,
                        "wait": tx.timing.ttfb_ms,
                        "receive": (tx.timing.total_ms - tx.timing.ttfb_ms).max(0.0),
                    }
                })
            })
            .collect();
        serde_json::to_string_pretty(&serde_json::json!({
            "log": {
                "version": "1.2",
                "creator": { "name": "ferro-devtools", "version": env!("CARGO_PKG_VERSION") },
                "entries": entries,
            }
        }))
        .unwrap_or_else(|_| "{\"log\":{\"version\":\"1.2\",\"entries\":[]}}".to_owned())
    }

    /// Clears all retained transactions.
    pub fn clear(&mut self) {
        self.transactions.clear();
    }
}

fn sanitize_headers(headers: Vec<NetworkHeader>) -> Vec<NetworkHeader> {
    headers
        .into_iter()
        .map(|mut header| {
            let lower = header.name.to_ascii_lowercase();
            if matches!(
                lower.as_str(),
                "authorization" | "proxy-authorization" | "cookie" | "set-cookie" | "x-api-key"
            ) {
                "<redacted>".clone_into(&mut header.value);
            }
            header
        })
        .collect()
}

fn matches_filter(tx: &NetworkTransaction, filter: &NetworkFilter) -> bool {
    if let Some(host) = &filter.host
        && !tx
            .host
            .to_ascii_lowercase()
            .contains(&host.to_ascii_lowercase())
    {
        return false;
    }
    if let Some(method) = &filter.method
        && !tx.method.eq_ignore_ascii_case(method)
    {
        return false;
    }
    if let Some(min_status) = filter.min_status
        && tx.status.is_none_or(|code| code < min_status)
    {
        return false;
    }
    if let Some(max_status) = filter.max_status
        && tx.status.is_none_or(|code| code > max_status)
    {
        return false;
    }
    if filter.errors_only && tx.error.is_none() && tx.status.is_none_or(|code| code < 400) {
        return false;
    }
    if let Some(min_ms) = filter.min_duration_ms {
        #[allow(
            clippy::cast_precision_loss,
            reason = "millisecond filter thresholds fit cleanly in f64"
        )]
        if tx.timing.total_ms < (min_ms as f64) {
            return false;
        }
    }
    true
}

fn extract_host(url: &str) -> String {
    let without_scheme = url
        .split_once("://")
        .map_or(url, |(_, remainder)| remainder);
    without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .to_owned()
}

/// Active OS network socket observed for a process.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SocketConnection {
    /// Process command name.
    pub process: String,
    /// Process identifier.
    pub pid: u32,
    /// Protocol (`TCP` or `UDP`).
    pub protocol: String,
    /// Local and/or remote endpoint descriptor.
    pub endpoint: String,
    /// Socket state (`LISTEN`, `ESTABLISHED`, etc.) if reported.
    pub state: Option<String>,
}

/// Inspects active OS TCP/UDP sockets, optionally filtered by `pid`, `process_name`, or `LISTEN` state.
///
/// # Errors
///
/// Returns an error if the platform socket probe tool cannot run.
pub fn inspect_sockets(
    pid: Option<u32>,
    process_name: Option<&str>,
    listening_only: bool,
) -> Result<Vec<SocketConnection>, String> {
    if cfg!(any(target_os = "macos", target_os = "linux")) {
        let mut command = Command::new("lsof");
        command.args(["-i", "-n", "-P"]);
        if let Some(pid) = pid {
            command.args(["-p", &pid.to_string()]);
        }
        let output = command
            .output()
            .map_err(|error| format!("could not execute lsof: {error}"))?;
        let text = String::from_utf8_lossy(&output.stdout);
        let mut items = parse_lsof_network(&text);
        if let Some(filter) = process_name {
            let needle = filter.to_ascii_lowercase();
            items.retain(|conn| conn.process.to_ascii_lowercase().contains(&needle));
        }
        if listening_only {
            items.retain(|conn| conn.state.as_deref() == Some("LISTEN"));
        }
        return Ok(items);
    }
    Ok(Vec::new())
}

/// Parses `lsof -i -n -P` tabular output into [`SocketConnection`] records.
#[must_use]
pub fn parse_lsof_network(output: &str) -> Vec<SocketConnection> {
    let mut results = Vec::new();
    for line in output.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 9 {
            continue;
        }
        let process = cols[0].to_owned();
        let Ok(pid) = cols[1].parse::<u32>() else {
            continue;
        };
        let protocol = cols[7].to_owned();
        let last = cols[cols.len() - 1];
        let (endpoint, state) = if last.starts_with('(') && last.ends_with(')') && cols.len() >= 10
        {
            (
                cols[cols.len() - 2].to_owned(),
                Some(last.trim_matches(&['(', ')'][..]).to_owned()),
            )
        } else {
            (last.to_owned(), None)
        };
        results.push(SocketConnection {
            process,
            pid,
            protocol,
            endpoint,
            state,
        });
    }
    results
}

/// Live network probe result for an `http://`, `https://`, or `tcp://` endpoint.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetworkProbeResult {
    /// Probed URL.
    pub url: String,
    /// HTTP status code when an HTTP response was parsed.
    pub status: Option<u16>,
    /// Timing breakdown.
    pub timing: NetworkTiming,
    /// Response headers (sensitive values redacted).
    pub headers: Vec<NetworkHeader>,
    /// Response payload byte count read.
    pub response_bytes: u64,
}

/// Probes a live `http://`, `https://`, or `tcp://` endpoint and measures connection latencies.
///
/// # Errors
///
/// Returns an error if the URL is invalid, unreachable, or times out.
pub fn probe_endpoint(url: &str, timeout: Duration) -> Result<NetworkProbeResult, String> {
    let trimmed = url.trim();
    if let Some(rest) = trimmed.strip_prefix("tcp://") {
        let host_port = rest.split('/').next().unwrap_or("");
        return probe_tcp(trimmed, host_port, timeout);
    }
    if let Some(rest) = trimmed.strip_prefix("http://") {
        return probe_http_plain(trimmed, rest, timeout);
    }
    if trimmed.starts_with("https://") {
        return probe_https_via_curl(trimmed, timeout);
    }
    Err(format!(
        "unsupported probe URL `{trimmed}` (expected http://, https://, or tcp://)"
    ))
}

fn probe_tcp(url: &str, host_port: &str, timeout: Duration) -> Result<NetworkProbeResult, String> {
    let dns_start = Instant::now();
    let addr = host_port
        .to_socket_addrs()
        .map_err(|error| format!("DNS resolution failed for `{host_port}`: {error}"))?
        .next()
        .ok_or_else(|| format!("no socket address resolved for `{host_port}`"))?;
    let dns_ms = dns_start.elapsed().as_secs_f64() * 1000.0;

    let conn_start = Instant::now();
    TcpStream::connect_timeout(&addr, timeout)
        .map_err(|error| format!("TCP connect failed for `{host_port}`: {error}"))?;
    let connect_ms = conn_start.elapsed().as_secs_f64() * 1000.0;
    let total_ms = dns_ms + connect_ms;

    Ok(NetworkProbeResult {
        url: url.to_owned(),
        status: None,
        timing: NetworkTiming {
            dns_ms,
            connect_ms,
            tls_ms: 0.0,
            ttfb_ms: 0.0,
            total_ms,
        },
        headers: Vec::new(),
        response_bytes: 0,
    })
}

fn probe_http_plain(
    full_url: &str,
    without_scheme: &str,
    timeout: Duration,
) -> Result<NetworkProbeResult, String> {
    let (authority, path) = match without_scheme.find('/') {
        Some(idx) => (&without_scheme[..idx], &without_scheme[idx..]),
        None => (without_scheme, "/"),
    };
    let host_port = if authority.contains(':') {
        authority.to_owned()
    } else {
        format!("{authority}:80")
    };
    let total_start = Instant::now();
    let dns_start = Instant::now();
    let addr = host_port
        .to_socket_addrs()
        .map_err(|error| format!("DNS resolution failed for `{host_port}`: {error}"))?
        .next()
        .ok_or_else(|| format!("no socket address resolved for `{host_port}`"))?;
    let dns_ms = dns_start.elapsed().as_secs_f64() * 1000.0;

    let conn_start = Instant::now();
    let mut stream = TcpStream::connect_timeout(&addr, timeout)
        .map_err(|error| format!("TCP connect failed for `{host_port}`: {error}"))?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let connect_ms = conn_start.elapsed().as_secs_f64() * 1000.0;

    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {authority}\r\nUser-Agent: ferro-devtools\r\nConnection: close\r\n\r\n"
    );
    let req_start = Instant::now();
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("HTTP write failed: {error}"))?;

    let mut raw = Vec::new();
    let mut first_byte = [0_u8; 1];
    let n = stream
        .read(&mut first_byte)
        .map_err(|error| format!("HTTP read failed: {error}"))?;
    let ttfb_ms = req_start.elapsed().as_secs_f64() * 1000.0;
    if n > 0 {
        raw.push(first_byte[0]);
        let _ = stream.read_to_end(&mut raw);
    }
    let total_ms = total_start.elapsed().as_secs_f64() * 1000.0;

    let text = String::from_utf8_lossy(&raw);
    let (status, headers, body_len) = parse_http_response(&text, raw.len());

    Ok(NetworkProbeResult {
        url: full_url.to_owned(),
        status,
        timing: NetworkTiming {
            dns_ms,
            connect_ms,
            tls_ms: 0.0,
            ttfb_ms,
            total_ms,
        },
        headers: sanitize_headers(headers),
        response_bytes: body_len as u64,
    })
}

fn probe_https_via_curl(url: &str, timeout: Duration) -> Result<NetworkProbeResult, String> {
    let timeout_secs = timeout.as_secs_f64().max(0.5);
    let output = Command::new("curl")
        .args([
            "-sS",
            "-o",
            "/dev/null",
            "--max-time",
            &format!("{timeout_secs:.2}"),
            "-w",
            "%{http_code}\t%{time_namelookup}\t%{time_connect}\t%{time_appconnect}\t%{time_starttransfer}\t%{time_total}\t%{size_download}",
            url,
        ])
        .output()
        .map_err(|error| format!("could not run curl probe: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "HTTPS probe failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let line = String::from_utf8_lossy(&output.stdout);
    let cols: Vec<&str> = line.trim().split('\t').collect();
    if cols.len() < 7 {
        return Err(format!("unexpected curl probe output `{}`", line.trim()));
    }
    let status = cols[0].parse::<u16>().ok().filter(|&code| code > 0);
    let dns_ms = cols[1].parse::<f64>().unwrap_or(0.0) * 1000.0;
    let connect_ms = cols[2].parse::<f64>().unwrap_or(0.0) * 1000.0;
    let appconnect_ms = cols[3].parse::<f64>().unwrap_or(0.0) * 1000.0;
    let tls_ms = (appconnect_ms - connect_ms).max(0.0);
    let ttfb_ms = cols[4].parse::<f64>().unwrap_or(0.0) * 1000.0;
    let total_ms = cols[5].parse::<f64>().unwrap_or(0.0) * 1000.0;
    let response_bytes = cols[6].parse::<u64>().unwrap_or(0);

    Ok(NetworkProbeResult {
        url: url.to_owned(),
        status,
        timing: NetworkTiming {
            dns_ms,
            connect_ms,
            tls_ms,
            ttfb_ms,
            total_ms,
        },
        headers: Vec::new(),
        response_bytes,
    })
}

fn parse_http_response(
    raw_text: &str,
    total_bytes: usize,
) -> (Option<u16>, Vec<NetworkHeader>, usize) {
    let (head, body_len) = raw_text.split_once("\r\n\r\n").map_or_else(
        || (raw_text, total_bytes),
        |(head, body)| (head, body.len()),
    );
    let mut lines = head.lines();
    let status = lines
        .next()
        .and_then(|status_line| status_line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok());
    let headers = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some(NetworkHeader {
                name: name.trim().to_owned(),
                value: value.trim().to_owned(),
            })
        })
        .collect();
    (status, headers, body_len)
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
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    use super::{
        NetworkFilter, NetworkHeader, NetworkInspector, NetworkTiming, parse_lsof_network,
        probe_endpoint,
    };

    #[test]
    fn redacts_sensitive_headers_and_exports_har() {
        let mut inspector = NetworkInspector::new(16);
        inspector.record(
            "get",
            "https://api.ferro.dev/v1/workspaces",
            Some(200),
            vec![
                NetworkHeader {
                    name: "Authorization".to_owned(),
                    value: "Bearer secret-token".to_owned(),
                },
                NetworkHeader {
                    name: "Accept".to_owned(),
                    value: "application/json".to_owned(),
                },
            ],
            vec![NetworkHeader {
                name: "Set-Cookie".to_owned(),
                value: "sid=xyz".to_owned(),
            }],
            0,
            512,
            NetworkTiming {
                dns_ms: 2.0,
                connect_ms: 4.0,
                tls_ms: 8.0,
                ttfb_ms: 20.0,
                total_ms: 24.0,
            },
            None,
        );
        inspector.record(
            "POST",
            "http://127.0.0.1:9999/fail",
            Some(500),
            vec![],
            vec![],
            128,
            0,
            NetworkTiming {
                total_ms: 120.0,
                ..NetworkTiming::default()
            },
            Some("server error".to_owned()),
        );

        let filtered = inspector.query(&NetworkFilter {
            errors_only: true,
            ..NetworkFilter::default()
        });
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].status, Some(500));

        let har = inspector.to_har_json();
        assert!(!har.contains("secret-token"));
        assert!(!har.contains("sid=xyz"));
        assert!(har.contains("<redacted>"));
        assert_eq!(inspector.summary().total_requests, 2);
    }

    #[test]
    fn parses_lsof_network_rows() {
        let output = "\
COMMAND   PID  USER   FD   TYPE DEVICE SIZE/OFF NODE NAME
ferro     123  shayn  10u  IPv4 0x01      0t0  TCP 127.0.0.1:7777 (LISTEN)
ferro     123  shayn  11u  IPv4 0x02      0t0  TCP 127.0.0.1:50000->127.0.0.1:7777 (ESTABLISHED)
";
        let sockets = parse_lsof_network(output);
        assert_eq!(sockets.len(), 2);
        assert_eq!(sockets[0].state.as_deref(), Some("LISTEN"));
        assert_eq!(sockets[1].state.as_deref(), Some("ESTABLISHED"));
    }

    #[test]
    fn probes_local_http_server() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("addr").port();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0_u8; 256];
                let _ = stream.read(&mut buf);
                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nX-Ferro: ok\r\n\r\nok");
            }
        });

        let result = probe_endpoint(
            &format!("http://127.0.0.1:{port}/health"),
            Duration::from_secs(2),
        )
        .expect("probe localhost");
        assert_eq!(result.status, Some(200));
        assert_eq!(result.response_bytes, 2);
        assert!(result.headers.iter().any(|header| header.name == "X-Ferro"));
        let _ = handle.join();
    }
}
