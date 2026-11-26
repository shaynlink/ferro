//! Minimal loopback-only HTTP client for talking to running `Ferro` `DevTools` endpoints.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use ferro_devtools::DevToolsDiscovery;

/// Scans `std::env::temp_dir()` for active `ferro-devtools-*.json` discovery files.
#[must_use]
pub(crate) fn discover_devtools_targets() -> Vec<DevToolsDiscovery> {
    let Ok(entries) = fs::read_dir(std::env::temp_dir()) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("ferro-devtools-") || !name.ends_with(".json") {
            continue;
        }
        let Ok(raw) = fs::read_to_string(entry.path()) else {
            continue;
        };
        if let Ok(item) = serde_json::from_str::<DevToolsDiscovery>(&raw)
            && http_get_text(&item.url, "/health").is_ok()
        {
            found.push(item);
        }
    }
    found.sort_by_key(|item| item.pid);
    found
}

/// Resolves an explicit `--url`, matching `--pid`, or the sole active discovered target.
pub(crate) fn resolve_devtools_url(
    explicit_url: Option<&str>,
    pid: Option<u32>,
) -> Result<String, String> {
    if let Some(url) = explicit_url {
        return validate_loopback_base_url(url);
    }
    let targets = discover_devtools_targets();
    if let Some(pid) = pid {
        return targets
            .into_iter()
            .find(|item| item.pid == pid)
            .map(|item| item.url)
            .ok_or_else(|| format!("no running Ferro DevTools endpoint found for pid {pid}"));
    }
    match targets.as_slice() {
        [] => Err("no running Ferro DevTools instance found (launch with `ferro dev` or `ferro debug launch`)".to_owned()),
        [single] => Ok(single.url.clone()),
        many => {
            let choices: Vec<String> = many
                .iter()
                .map(|item| format!("pid {} ({}) -> {}", item.pid, item.app_name, item.url))
                .collect();
            Err(format!(
                "multiple DevTools instances running; pass --pid or --url: {}",
                choices.join(", ")
            ))
        }
    }
}

pub(crate) fn validate_loopback_base_url(url: &str) -> Result<String, String> {
    let trimmed = url.trim().trim_end_matches('/');
    let rest = trimmed
        .strip_prefix("http://")
        .ok_or_else(|| format!("DevTools URL `{trimmed}` must start with http://"))?;
    let host = rest.split('/').next().unwrap_or("");
    if host.starts_with("127.0.0.1:")
        || host == "127.0.0.1"
        || host.starts_with("localhost:")
        || host == "localhost"
    {
        Ok(trimmed.to_owned())
    } else {
        Err(format!("refusing non-loopback DevTools URL `{trimmed}`"))
    }
}

pub(crate) fn http_get_text(base_url: &str, path: &str) -> Result<String, String> {
    http_request(base_url, "GET", path, "")
}

pub(crate) fn http_post_json(base_url: &str, path: &str, body: &str) -> Result<String, String> {
    http_request(base_url, "POST", path, body)
}

fn http_request(base_url: &str, method: &str, path: &str, body: &str) -> Result<String, String> {
    let clean = validate_loopback_base_url(base_url)?;
    let authority = clean.strip_prefix("http://").unwrap_or(&clean);
    let mut stream = TcpStream::connect(authority)
        .map_err(|error| format!("could not connect to DevTools at {clean}: {error}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));

    let request = if method == "POST" {
        format!(
            "POST {path} HTTP/1.1\r\nHost: {authority}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    } else {
        format!("GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n")
    };
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("DevTools request write failed: {error}"))?;
    let mut raw = String::new();
    stream
        .read_to_string(&mut raw)
        .map_err(|error| format!("DevTools response read failed: {error}"))?;

    let (head, resp_body) = raw
        .split_once("\r\n\r\n")
        .ok_or_else(|| "malformed HTTP response from DevTools".to_owned())?;
    let status_line = head.lines().next().unwrap_or("");
    let code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|tok| tok.parse::<u16>().ok())
        .unwrap_or(0);
    if (200..300).contains(&code) {
        Ok(resp_body.to_owned())
    } else {
        Err(format!(
            "DevTools {method} {path} returned {status_line}: {resp_body}"
        ))
    }
}
