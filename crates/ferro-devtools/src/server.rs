//! Loopback-only (`127.0.0.1`) `DevTools` JSON/HTTP server and self-contained dashboard.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ferro_testkit::UiTree;
use serde::{Deserialize, Serialize};

use crate::debug::DebugSession;
use crate::network::{NetworkFilter, NetworkInspector};
use crate::profiler::{Profiler, render_svg_flamegraph};
use crate::telemetry::TelemetryCollector;

/// Shared mutable `DevTools` state observed by a running Ferro application and [`LocalDevToolsServer`].
#[derive(Debug)]
pub struct DevToolsState {
    /// Reducer event trace and breakpoint state.
    pub debug: DebugSession,
    /// Span and frame profiler.
    pub profiler: Profiler,
    /// HTTP/IPC network transaction recorder.
    pub network: NetworkInspector,
    /// Most recently captured UI hierarchy.
    pub ui_tree: Option<UiTree>,
    /// Count of hot-reload triggers received via `/dev/reload`.
    pub reload_requests: u64,
    /// Queued UI action payloads posted to `/ui/action`.
    pub pending_actions: Vec<String>,
}

impl DevToolsState {
    /// Creates shared `DevTools` state for an application.
    #[must_use]
    pub fn new(app_name: impl Into<String>, app_id: impl Into<String>) -> Self {
        Self {
            debug: DebugSession::new(app_name, app_id, 500),
            profiler: Profiler::new(2000, 1000, 60),
            network: NetworkInspector::new(500),
            ui_tree: None,
            reload_requests: 0,
            pending_actions: Vec::new(),
        }
    }
}

/// Optional callback invoked on the main loop when `/dev/reload` or `/ui/action` arrives.
pub type WakeCallback = Arc<dyn Fn() + Send + Sync>;

/// Running loopback `DevTools` server handle. Stops cleanly on drop or [`Self::stop`].
pub struct LocalDevToolsServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl LocalDevToolsServer {
    /// Binds strictly to `127.0.0.1:<port>` (pass `0` for an ephemeral free port) and starts serving.
    ///
    /// # Errors
    ///
    /// Returns an error if the loopback TCP socket cannot bind.
    pub fn start(
        port: u16,
        state: Arc<Mutex<DevToolsState>>,
        on_wake: Option<WakeCallback>,
    ) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|error| {
            format!("could not bind DevTools server on 127.0.0.1:{port}: {error}")
        })?;
        let addr = listener
            .local_addr()
            .map_err(|error| format!("could not read DevTools socket address: {error}"))?;
        listener.set_nonblocking(true).map_err(|error| {
            format!("could not configure nonblocking DevTools listener: {error}")
        })?;

        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            let mut collector = TelemetryCollector::new();
            while !stop_flag.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = handle_connection(stream, &state, &mut collector, on_wake.as_ref());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(25));
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            addr,
            stop,
            handle: Some(handle),
        })
    }

    /// Returns the bound loopback socket address (`127.0.0.1:<port>`).
    #[must_use]
    pub const fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Returns the base HTTP URL (`http://127.0.0.1:<port>`).
    #[must_use]
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Signals the background listener thread to exit and waits for it.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for LocalDevToolsServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Discovery metadata written by a running Ferro app when `DevTools` are enabled.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DevToolsDiscovery {
    /// OS process identifier.
    pub pid: u32,
    /// Application name.
    pub app_name: String,
    /// Loopback `DevTools` base URL.
    pub url: String,
}

fn handle_connection(
    mut stream: TcpStream,
    state: &Arc<Mutex<DevToolsState>>,
    collector: &mut TelemetryCollector,
    on_wake: Option<&WakeCallback>,
) -> std::io::Result<()> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_nonblocking(false);
    let mut buffer = [0_u8; 8192];
    let read_len = stream.read(&mut buffer)?;
    if read_len == 0 {
        return Ok(());
    }
    let request = String::from_utf8_lossy(&buffer[..read_len]);
    let (head, body) = request.split_once("\r\n\r\n").unwrap_or((&request, ""));
    let request_line = head.lines().next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let raw_target = parts.next().unwrap_or("/");
    let path = raw_target.split('?').next().unwrap_or(raw_target);

    let (status, content_type, response_body) =
        route_request(method, path, body, state, collector, on_wake)?;

    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        response_body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(response_body.as_bytes())?;
    stream.flush()
}

fn route_request(
    method: &str,
    path: &str,
    body: &str,
    state: &Arc<Mutex<DevToolsState>>,
    collector: &mut TelemetryCollector,
    on_wake: Option<&WakeCallback>,
) -> std::io::Result<(&'static str, &'static str, String)> {
    let reply = match (method, path) {
        ("GET", "/") => (
            "200 OK",
            "text/html; charset=utf-8",
            render_dashboard_html(),
        ),
        ("GET", "/health") => (
            "200 OK",
            "application/json",
            format!("{{\"status\":\"ok\",\"pid\":{}}}", std::process::id()),
        ),
        ("GET", "/state" | "/trace") => {
            let guard = lock_state(state)?;
            ("200 OK", "application/json", guard.debug.to_json_pretty())
        }
        ("GET", "/profile" | "/profile/trace.json" | "/profile/flamegraph.svg") => {
            serve_profile_route(path, state)?
        }
        ("GET", "/network" | "/network/har") => serve_network_route(path, state)?,
        ("GET", "/telemetry") => {
            let snapshot = collector.collect(Some(std::process::id()), Duration::ZERO);
            (
                "200 OK",
                "application/json",
                serde_json::to_string_pretty(&snapshot).unwrap_or_else(|_| "{}".to_owned()),
            )
        }
        ("GET", "/ui") | ("POST", "/ui/action" | "/dev/reload") => {
            serve_control_route(method, path, body, state, on_wake)?
        }
        _ => (
            "404 Not Found",
            "application/json",
            "{\"error\":\"not found\"}".to_owned(),
        ),
    };
    Ok(reply)
}

fn lock_state(
    state: &Arc<Mutex<DevToolsState>>,
) -> std::io::Result<std::sync::MutexGuard<'_, DevToolsState>> {
    state
        .lock()
        .map_err(|_| std::io::Error::other("devtools lock poisoned"))
}

fn serve_profile_route(
    path: &str,
    state: &Arc<Mutex<DevToolsState>>,
) -> std::io::Result<(&'static str, &'static str, String)> {
    let guard = lock_state(state)?;
    Ok(match path {
        "/profile" => {
            let summary = guard.profiler.summarize();
            (
                "200 OK",
                "application/json",
                serde_json::to_string_pretty(&summary).unwrap_or_else(|_| "{}".to_owned()),
            )
        }
        "/profile/trace.json" => (
            "200 OK",
            "application/json",
            guard.profiler.to_chrome_trace_json(),
        ),
        _ => {
            let folded = guard.profiler.to_folded_stacks();
            let title = format!("{} Profiler Flamegraph", guard.debug.app_name());
            (
                "200 OK",
                "image/svg+xml",
                render_svg_flamegraph(&title, &folded),
            )
        }
    })
}

fn serve_network_route(
    path: &str,
    state: &Arc<Mutex<DevToolsState>>,
) -> std::io::Result<(&'static str, &'static str, String)> {
    let guard = lock_state(state)?;
    Ok(if path == "/network/har" {
        ("200 OK", "application/json", guard.network.to_har_json())
    } else {
        let transactions = guard.network.query(&NetworkFilter::default());
        let summary = guard.network.summary();
        (
            "200 OK",
            "application/json",
            serde_json::to_string_pretty(&serde_json::json!({
                "summary": summary,
                "transactions": transactions,
            }))
            .unwrap_or_else(|_| "{}".to_owned()),
        )
    })
}

fn serve_control_route(
    method: &str,
    path: &str,
    body: &str,
    state: &Arc<Mutex<DevToolsState>>,
    on_wake: Option<&WakeCallback>,
) -> std::io::Result<(&'static str, &'static str, String)> {
    let mut guard = lock_state(state)?;
    if method == "GET" && path == "/ui" {
        let payload = guard.ui_tree.as_ref().map_or_else(
            || "{\"ui_tree\":null}".to_owned(),
            |tree| serde_json::to_string_pretty(tree).unwrap_or_else(|_| "{}".to_owned()),
        );
        return Ok(("200 OK", "application/json", payload));
    }
    if path == "/ui/action" {
        guard.pending_actions.push(body.trim().to_owned());
        drop(guard);
        if let Some(wake) = on_wake {
            wake();
        }
        return Ok((
            "202 Accepted",
            "application/json",
            "{\"queued\":true}".to_owned(),
        ));
    }
    guard.reload_requests += 1;
    let count = guard.reload_requests;
    drop(guard);
    if let Some(wake) = on_wake {
        wake();
    }
    Ok((
        "200 OK",
        "application/json",
        format!("{{\"reloaded\":true,\"count\":{count}}}"),
    ))
}

fn render_dashboard_html() -> String {
    r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8"/>
<title>Ferro DevTools</title>
<style>
  body { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; background: #0d1117; color: #e6edf3; margin: 24px; }
  h1 { margin: 0 0 12px 0; font-size: 20px; color: #58a6ff; }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); gap: 12px; margin-bottom: 16px; }
  .card { background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 12px; }
  .card h2 { margin: 0 0 8px 0; font-size: 13px; color: #8b949e; text-transform: uppercase; }
  pre { white-space: pre-wrap; word-break: break-word; margin: 0; font-size: 12px; color: #c9d1d9; max-height: 280px; overflow: auto; }
  button { background: #238636; color: #fff; border: 0; border-radius: 6px; padding: 6px 12px; cursor: pointer; font-family: inherit; }
</style>
</head>
<body>
<h1>Ferro DevTools Inspector</h1>
<p><button onclick="refreshAll()">Refresh</button> <button onclick="triggerReload()">Trigger Hot Reload</button></p>
<div class="grid">
  <div class="card"><h2>System & Hardware Telemetry</h2><pre id="telemetry">Loading…</pre></div>
  <div class="card"><h2>State & Event Trace</h2><pre id="trace">Loading…</pre></div>
  <div class="card"><h2>Profiler Summary</h2><pre id="profile">Loading…</pre></div>
  <div class="card"><h2>Network Inspector</h2><pre id="network">Loading…</pre></div>
  <div class="card"><h2>UI Tree</h2><pre id="ui">Loading…</pre></div>
</div>
<script>
async function fetchText(path, el) {
  try {
    const res = await fetch(path);
    document.getElementById(el).textContent = await res.text();
  } catch (e) {
    document.getElementById(el).textContent = String(e);
  }
}
function refreshAll() {
  fetchText('/telemetry', 'telemetry');
  fetchText('/trace', 'trace');
  fetchText('/profile', 'profile');
  fetchText('/network', 'network');
  fetchText('/ui', 'ui');
}
async function triggerReload() {
  await fetch('/dev/reload', { method: 'POST' });
  refreshAll();
}
refreshAll();
setInterval(refreshAll, 2000);
</script>
</body>
</html>
"#.to_owned()
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::{DevToolsState, LocalDevToolsServer};
    use crate::network::NetworkTiming;

    fn http_get(addr: std::net::SocketAddr, req: &str) -> String {
        let mut stream = TcpStream::connect(addr).expect("connect devtools");
        stream.write_all(req.as_bytes()).expect("write request");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read response");
        response
    }

    #[test]
    fn serves_devtools_endpoints_and_triggers_reload_wake() {
        let state = Arc::new(Mutex::new(DevToolsState::new("Atlas", "dev.ferro.atlas")));
        {
            let mut guard = state.lock().expect("lock");
            guard
                .debug
                .record_dispatch("Launched", vec!["Render".to_owned()], 35, None, None);
            guard.profiler.measure("reduce", "state", || 1 + 1);
            guard.network.record(
                "GET",
                "http://localhost:8080/api",
                Some(200),
                vec![],
                vec![],
                0,
                128,
                NetworkTiming::default(),
                None,
            );
        }
        let wakes = Arc::new(AtomicUsize::new(0));
        let wakes_clone = Arc::clone(&wakes);
        let mut server = LocalDevToolsServer::start(
            0,
            Arc::clone(&state),
            Some(Arc::new(move || {
                wakes_clone.fetch_add(1, Ordering::Relaxed);
            })),
        )
        .expect("start devtools server");

        let health = http_get(
            server.addr(),
            "GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n",
        );
        assert!(health.contains("200 OK"));
        assert!(health.contains("\"status\":\"ok\""));

        let svg = http_get(
            server.addr(),
            "GET /profile/flamegraph.svg HTTP/1.1\r\nHost: localhost\r\n\r\n",
        );
        assert!(svg.contains("<svg"));

        let har = http_get(
            server.addr(),
            "GET /network/har HTTP/1.1\r\nHost: localhost\r\n\r\n",
        );
        assert!(har.contains("\"version\": \"1.2\""));

        let reload = http_get(
            server.addr(),
            "POST /dev/reload HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n",
        );
        assert!(reload.contains("\"reloaded\":true"));
        assert_eq!(wakes.load(Ordering::Relaxed), 1);

        server.stop();
    }
}
