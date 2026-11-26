//! Event/effect trace recording, breakpoint inspection, bundle diagnostics, and crash reports.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// One recorded reducer/lifecycle transition inside a debug session.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EventTraceEntry {
    /// Monotonically increasing sequence identifier.
    pub sequence: u64,
    /// Unix epoch timestamp in milliseconds.
    pub timestamp_ms: u64,
    /// Human-readable event description.
    pub event: String,
    /// Ordered effects emitted by the reducer.
    pub effects: Vec<String>,
    /// Reducer duration in microseconds.
    pub duration_us: u64,
    /// Optional state summary or serialized representation after dispatch.
    pub state_summary: Option<String>,
    /// Optional window identifier associated with the transition.
    pub window_id: Option<String>,
}

/// Bounded in-memory event trace & breakpoint state for debugging a Ferro application.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DebugSession {
    app_name: String,
    app_id: String,
    max_entries: usize,
    next_sequence: u64,
    paused: bool,
    breakpoint_filters: Vec<String>,
    entries: VecDeque<EventTraceEntry>,
    last_state_summary: Option<String>,
}

impl DebugSession {
    /// Creates a new debug session retaining up to `max_entries` trace items.
    #[must_use]
    pub fn new(app_name: impl Into<String>, app_id: impl Into<String>, max_entries: usize) -> Self {
        Self {
            app_name: app_name.into(),
            app_id: app_id.into(),
            max_entries: max_entries.max(1),
            next_sequence: 1,
            paused: false,
            breakpoint_filters: Vec::new(),
            entries: VecDeque::new(),
            last_state_summary: None,
        }
    }

    /// Returns the application name bound to this session.
    #[must_use]
    pub fn app_name(&self) -> &str {
        &self.app_name
    }

    /// Returns the application identifier bound to this session.
    #[must_use]
    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    /// Returns whether trace capture is paused on a breakpoint.
    #[must_use]
    pub const fn is_paused(&self) -> bool {
        self.paused
    }

    /// Pauses or resumes breakpoint trapping.
    pub const fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Adds a substring breakpoint filter matched against formatted event names.
    pub fn add_breakpoint(&mut self, event_pattern: impl Into<String>) {
        let pattern = event_pattern.into();
        if !pattern.trim().is_empty() && !self.breakpoint_filters.contains(&pattern) {
            self.breakpoint_filters.push(pattern);
        }
    }

    /// Clears all configured breakpoint filters.
    pub fn clear_breakpoints(&mut self) {
        self.breakpoint_filters.clear();
    }

    /// Records a dispatched event and returns `true` if a configured breakpoint matched.
    pub fn record_dispatch(
        &mut self,
        event: impl Into<String>,
        effects: Vec<String>,
        duration_us: u64,
        state_summary: Option<String>,
        window_id: Option<String>,
    ) -> bool {
        let event = event.into();
        let hit_breakpoint = self
            .breakpoint_filters
            .iter()
            .any(|pattern| event.contains(pattern));
        if hit_breakpoint {
            self.paused = true;
        }
        if let Some(summary) = &state_summary {
            self.last_state_summary = Some(summary.clone());
        }
        let entry = EventTraceEntry {
            sequence: self.next_sequence,
            timestamp_ms: now_ms(),
            event,
            effects,
            duration_us,
            state_summary,
            window_id,
        };
        self.next_sequence += 1;
        if self.entries.len() == self.max_entries {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
        hit_breakpoint
    }

    /// Returns all retained trace entries in chronological order.
    #[must_use]
    pub fn entries(&self) -> Vec<EventTraceEntry> {
        self.entries.iter().cloned().collect()
    }

    /// Returns the most recently captured state summary, if any.
    #[must_use]
    pub fn last_state_summary(&self) -> Option<&str> {
        self.last_state_summary.as_deref()
    }

    /// Updates the latest state summary without adding an event entry.
    pub fn update_state_summary(&mut self, summary: impl Into<String>) {
        self.last_state_summary = Some(summary.into());
    }

    /// Clears all recorded trace entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Exports the debug session state and event trace as pretty JSON.
    #[must_use]
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_owned())
    }
}

/// Static diagnostic summary of a built macOS `.app` bundle.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BundleInspection {
    /// Bundle path on disk.
    pub bundle_path: String,
    /// Main Mach-O executable path if found.
    pub executable_path: Option<String>,
    /// Total recursive size of the `.app` bundle in bytes.
    pub total_size_bytes: u64,
    /// Main executable size in bytes.
    pub executable_size_bytes: Option<u64>,
    /// `CFBundleIdentifier` extracted from `Info.plist`.
    pub bundle_identifier: Option<String>,
    /// `CFBundleShortVersionString` extracted from `Info.plist`.
    pub version: Option<String>,
    /// `CFBundleVersion` extracted from `Info.plist`.
    pub build_number: Option<String>,
    /// Architectures reported by `lipo -archs`.
    pub architectures: Vec<String>,
    /// Code signature verification summary (`codesign --verify`).
    pub signature_status: String,
    /// Dynamic libraries linked by the main executable (`otool -L`).
    pub linked_libraries: Vec<String>,
}

/// Inspects a macOS `.app` bundle for metadata, architectures, linked frameworks, and signature.
///
/// # Errors
///
/// Returns an error if `bundle_path` does not exist or is not a directory.
pub fn inspect_macos_bundle(bundle_path: &Path) -> Result<BundleInspection, String> {
    if !bundle_path.is_dir() {
        return Err(format!(
            "bundle `{}` does not exist or is not a directory",
            bundle_path.display()
        ));
    }

    let total_size_bytes = directory_size_bytes(bundle_path);
    let plist_path = bundle_path.join("Contents/Info.plist");
    let plist_source = fs::read_to_string(&plist_path).unwrap_or_default();

    let bundle_identifier = extract_plist_string(&plist_source, "CFBundleIdentifier");
    let version = extract_plist_string(&plist_source, "CFBundleShortVersionString");
    let build_number = extract_plist_string(&plist_source, "CFBundleVersion");
    let executable_name = extract_plist_string(&plist_source, "CFBundleExecutable");

    let executable_path = executable_name
        .as_deref()
        .map(|name| bundle_path.join("Contents/MacOS").join(name))
        .filter(|path| path.is_file())
        .or_else(|| first_file_in(&bundle_path.join("Contents/MacOS")));

    let executable_size_bytes = executable_path
        .as_ref()
        .and_then(|path| fs::metadata(path).ok())
        .map(|meta| meta.len());

    let (architectures, linked_libraries) = match &executable_path {
        Some(exe) if cfg!(target_os = "macos") => (probe_lipo_archs(exe), probe_otool_libs(exe)),
        _ => (Vec::new(), Vec::new()),
    };

    let signature_status = if cfg!(target_os = "macos") {
        probe_codesign_status(bundle_path)
    } else {
        "unavailable on non-macOS host".to_owned()
    };

    Ok(BundleInspection {
        bundle_path: bundle_path.display().to_string(),
        executable_path: executable_path.map(|path| path.display().to_string()),
        total_size_bytes,
        executable_size_bytes,
        bundle_identifier,
        version,
        build_number,
        architectures,
        signature_status,
        linked_libraries,
    })
}

/// Summary of a crash report file discovered on disk.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CrashReportEntry {
    /// Filename of the crash report.
    pub file_name: String,
    /// Full path on disk.
    pub path: String,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Last modification timestamp in Unix epoch milliseconds.
    pub modified_ms: u64,
}

/// Lists recent crash report files (`.ips` or `.crash`) under `~/Library/Logs/DiagnosticReports`
/// on macOS, or an explicit directory when provided.
#[must_use]
pub fn list_crash_reports(
    directory: Option<&Path>,
    app_filter: Option<&str>,
    limit: usize,
) -> Vec<CrashReportEntry> {
    let default_dir = std::env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join("Library")
            .join("Logs")
            .join("DiagnosticReports")
    });
    let target_dir = directory.map(Path::to_path_buf).or(default_dir);
    let Some(dir) = target_dir else {
        return Vec::new();
    };
    let Ok(read_dir) = fs::read_dir(dir) else {
        return Vec::new();
    };

    let filter_lower = app_filter.map(str::to_ascii_lowercase);
    let mut entries = Vec::new();
    for entry in read_dir.flatten() {
        let path = entry.path();
        let Some(ext) = path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        if ext != "ips" && ext != "crash" {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if let Some(filter) = &filter_lower
            && !file_name.to_ascii_lowercase().contains(filter)
        {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let modified_ms = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |dur| u64::try_from(dur.as_millis()).unwrap_or(u64::MAX));
        entries.push(CrashReportEntry {
            file_name,
            path: path.display().to_string(),
            size_bytes: metadata.len(),
            modified_ms,
        });
    }
    entries.sort_by_key(|item| std::cmp::Reverse(item.modified_ms));
    entries.truncate(limit);
    entries
}

/// Computes total recursive file size in bytes beneath `path`.
#[must_use]
pub fn directory_size_bytes(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    let Ok(read_dir) = fs::read_dir(path) else {
        return 0;
    };
    read_dir
        .flatten()
        .map(|entry| directory_size_bytes(&entry.path()))
        .sum()
}

/// Extracts a `<key>...</key><string>...</string>` value from XML plist text.
#[must_use]
pub fn extract_plist_string(plist: &str, key: &str) -> Option<String> {
    let key_tag = format!("<key>{key}</key>");
    let rest = plist.split(&key_tag).nth(1)?;
    let start = rest.find("<string>")? + "<string>".len();
    let after = &rest[start..];
    let end = after.find("</string>")?;
    Some(after[..end].trim().to_owned())
}

fn first_file_in(dir: &Path) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.is_file())
}

fn probe_lipo_archs(executable: &Path) -> Vec<String> {
    let Ok(output) = Command::new("lipo")
        .args(["-archs"])
        .arg(executable)
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

fn probe_otool_libs(executable: &Path) -> Vec<String> {
    let Ok(output) = Command::new("otool").args(["-L"]).arg(executable).output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
        .collect()
}

fn probe_codesign_status(bundle: &Path) -> String {
    match Command::new("codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(bundle)
        .output()
    {
        Ok(output) if output.status.success() => "valid".to_owned(),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            if stderr.is_empty() {
                "unsigned or invalid".to_owned()
            } else {
                stderr
            }
        }
        Err(error) => format!("unavailable ({error})"),
    }
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
    use super::{DebugSession, extract_plist_string, inspect_macos_bundle, list_crash_reports};

    #[test]
    fn debug_session_records_events_and_triggers_breakpoints() {
        let mut session = DebugSession::new("Atlas", "dev.ferro.atlas", 2);
        assert!(!session.is_paused());
        session.add_breakpoint("OpenFile");

        assert!(!session.record_dispatch(
            "Launched",
            vec![],
            120,
            Some("Ready".to_owned()),
            Some("main".to_owned()),
        ));
        assert!(!session.is_paused());

        assert!(session.record_dispatch(
            "OpenFile",
            vec!["OpenFileDialog".to_owned()],
            45,
            Some("Picking".to_owned()),
            Some("main".to_owned()),
        ));
        assert!(session.is_paused());

        session.record_dispatch("Done", vec![], 10, None, None);
        let entries = session.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].event, "OpenFile");
        assert_eq!(entries[1].event, "Done");
        assert!(session.to_json_pretty().contains("OpenFileDialog"));
    }

    #[test]
    fn inspects_bundle_plist_and_filters_crash_reports() {
        let dir = tempfile::tempdir().expect("tempdir");
        let app = dir.path().join("Atlas.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).expect("create macos dir");
        std::fs::write(app.join("Contents/MacOS/atlas"), b"binary").expect("write exec");
        std::fs::write(
            app.join("Contents/Info.plist"),
            "<dict><key>CFBundleIdentifier</key><string>dev.ferro.atlas</string><key>CFBundleExecutable</key><string>atlas</string><key>CFBundleShortVersionString</key><string>1.0.0</string><key>CFBundleVersion</key><string>7</string></dict>",
        )
        .expect("write plist");

        let info = inspect_macos_bundle(&app).expect("inspect bundle");
        assert_eq!(info.bundle_identifier.as_deref(), Some("dev.ferro.atlas"));
        assert_eq!(info.version.as_deref(), Some("1.0.0"));
        assert_eq!(info.build_number.as_deref(), Some("7"));
        assert_eq!(info.executable_size_bytes, Some(6));

        let crash_dir = dir.path().join("crashes");
        std::fs::create_dir_all(&crash_dir).expect("crash dir");
        std::fs::write(crash_dir.join("Atlas-2026.ips"), "{}").expect("crash 1");
        std::fs::write(crash_dir.join("Other-2026.crash"), "{}").expect("crash 2");
        let filtered = list_crash_reports(Some(&crash_dir), Some("atlas"), 10);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].file_name, "Atlas-2026.ips");

        assert_eq!(
            extract_plist_string("<key>K</key>\n  <string>V</string>", "K").as_deref(),
            Some("V")
        );
    }
}
