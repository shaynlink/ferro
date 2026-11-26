//! Developer debugging commands (`launch`, `attach`, `inspect`, `logs`, `state`).

use std::path::PathBuf;
use std::process::Command as ProcessCommand;

use clap::Subcommand;
use ferro_devtools::{inspect_macos_bundle, list_crash_reports};
use ferro_sdk::Target;

use crate::http_client::{http_get_text, resolve_devtools_url};
use crate::{BuildOptions, build_project};

#[derive(Debug, Subcommand)]
pub(crate) enum DebugCommand {
    /// Build and launch a Ferro target with `DevTools` diagnostics or under LLDB.
    Launch {
        /// Project directory containing ferro.toml.
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Target to launch.
        #[arg(long, default_value = "macos")]
        target: Target,
        /// Launch under interactive `lldb`.
        #[arg(long)]
        lldb: bool,
        /// Automatically open the native `DevTools` HUD window on launch.
        #[arg(long)]
        hud: bool,
        /// Bind the loopback `DevTools` server to a specific localhost port.
        #[arg(long)]
        devtools_port: Option<u16>,
    },
    /// Attach LLDB or collect a thread backtrace snapshot for a running process.
    Attach {
        /// Running process ID.
        #[arg(long)]
        pid: u32,
        /// Capture all thread backtraces non-interactively and detach immediately.
        #[arg(long)]
        backtrace: bool,
    },
    /// Inspect a built `.app` bundle (Info.plist, architectures, signature, linked libraries).
    Inspect {
        /// Path to an `.app` bundle, or omit to inspect the project's debug macOS bundle.
        #[arg(long)]
        bundle: Option<PathBuf>,
        /// Project directory used when `--bundle` is omitted.
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Inspect recent macOS crash reports and optional unified system log lines.
    Logs {
        /// Optional application name filter.
        #[arg(long)]
        app: Option<String>,
        /// Maximum crash reports to list.
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Query macOS unified logs (`log show --last <duration>`).
        #[arg(long)]
        system_log: bool,
        /// Lookback window for `--system-log` (e.g. `10m`, `1h`).
        #[arg(long, default_value = "10m")]
        last: String,
        /// Emit JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Fetch live state, reducer event trace, and UI snapshot from a running Ferro `DevTools` endpoint.
    State {
        /// Explicit loopback `DevTools` URL (e.g. `http://127.0.0.1:51234`).
        #[arg(long)]
        url: Option<String>,
        /// Process ID of a running Ferro app with `DevTools` enabled.
        #[arg(long)]
        pid: Option<u32>,
        /// Emit raw JSON from `/trace`.
        #[arg(long)]
        json: bool,
    },
}

pub(crate) fn run(command: DebugCommand) -> Result<(), String> {
    match command {
        DebugCommand::Launch {
            project,
            target,
            lldb,
            hud,
            devtools_port,
        } => launch_debug(&project, target, lldb, hud, devtools_port),
        DebugCommand::Attach { pid, backtrace } => attach_lldb(pid, backtrace),
        DebugCommand::Inspect { bundle, project } => {
            inspect_bundle_cmd(bundle.as_deref(), &project)
        }
        DebugCommand::Logs {
            app,
            limit,
            system_log,
            last,
            json,
        } => inspect_logs(app.as_deref(), limit, system_log, &last, json),
        DebugCommand::State { url, pid, json } => inspect_live_state(url.as_deref(), pid, json),
    }
}

fn launch_debug(
    project: &PathBuf,
    target: Target,
    lldb: bool,
    hud: bool,
    devtools_port: Option<u16>,
) -> Result<(), String> {
    let bundle = build_project(project, target, BuildOptions::default())?;
    if lldb {
        let status = ProcessCommand::new("lldb")
            .arg(&bundle.executable)
            .current_dir(project)
            .env("FERRO_DEVTOOLS", "1")
            .status()
            .map_err(|error| format!("could not launch lldb: {error}"))?;
        if status.success() {
            return Ok(());
        }
        return Err(format!("lldb exited with {status}"));
    }

    let mut cmd = ProcessCommand::new(&bundle.executable);
    cmd.current_dir(project).env("FERRO_DEVTOOLS", "1");
    if hud {
        cmd.env("FERRO_DEVTOOLS_HUD", "1");
    }
    if let Some(port) = devtools_port {
        cmd.env("FERRO_DEVTOOLS_PORT", port.to_string());
    }
    let status = cmd
        .status()
        .map_err(|error| format!("could not launch {}: {error}", bundle.path.display()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("debug target exited unsuccessfully ({status})"))
    }
}

fn attach_lldb(pid: u32, backtrace: bool) -> Result<(), String> {
    let mut cmd = ProcessCommand::new("lldb");
    cmd.args(["-p", &pid.to_string()]);
    if backtrace {
        cmd.args([
            "--batch",
            "-o",
            "thread backtrace all",
            "-o",
            "detach",
            "-o",
            "quit",
        ]);
    }
    let status = cmd
        .status()
        .map_err(|error| format!("could not run lldb: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("lldb attach failed ({status})"))
    }
}

fn inspect_bundle_cmd(
    bundle: Option<&std::path::Path>,
    project: &std::path::Path,
) -> Result<(), String> {
    let resolved = if let Some(path) = bundle {
        path.to_path_buf()
    } else {
        let (manifest, _) = crate::load_macos_project(project, Target::Macos)?;
        let target_root = crate::cargo_target_directory(project);
        let name = crate::safe_bundle_name(manifest.application().name());
        target_root
            .join("ferro/macos/debug")
            .join(format!("{name}.app"))
    };
    let info = inspect_macos_bundle(&resolved)?;
    let json = serde_json::to_string_pretty(&info)
        .map_err(|error| format!("could not encode bundle report: {error}"))?;
    println!("{json}");
    Ok(())
}

fn inspect_logs(
    app: Option<&str>,
    limit: usize,
    system_log: bool,
    last: &str,
    json: bool,
) -> Result<(), String> {
    let reports = list_crash_reports(None, app, limit);
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&reports)
                .map_err(|error| format!("could not encode logs JSON: {error}"))?
        );
    } else if reports.is_empty() {
        println!("No matching crash reports found.");
    } else {
        println!("Recent crash reports:");
        for item in &reports {
            println!(
                "  - {} ({} B) [{}]",
                item.file_name, item.size_bytes, item.path
            );
        }
    }

    if system_log && cfg!(target_os = "macos") {
        let mut cmd = ProcessCommand::new("log");
        cmd.args(["show", "--last", last, "--style", "compact"]);
        if let Some(app_name) = app {
            cmd.args([
                "--predicate",
                &format!("process CONTAINS[c] \"{app_name}\""),
            ]);
        }
        let output = cmd
            .output()
            .map_err(|error| format!("could not run macOS `log show`: {error}"))?;
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text
            .lines()
            .rev()
            .take(40)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            println!("{line}");
        }
    }
    Ok(())
}

fn inspect_live_state(url: Option<&str>, pid: Option<u32>, json: bool) -> Result<(), String> {
    let base = resolve_devtools_url(url, pid)?;
    let body = http_get_text(&base, "/trace")?;
    if json {
        println!("{body}");
    } else {
        let session: ferro_devtools::DebugSession = serde_json::from_str(&body)
            .map_err(|error| format!("invalid DevTools trace response: {error}"))?;
        println!(
            "Ferro Debug State — {} ({}) [paused={}]",
            session.app_name(),
            session.app_id(),
            session.is_paused()
        );
        if let Some(summary) = session.last_state_summary() {
            println!("Latest state: {summary}");
        }
        for entry in session.entries() {
            println!(
                "  #{:<4} {:>5} us  {} -> {:?}",
                entry.sequence, entry.duration_us, entry.event, entry.effects
            );
        }
    }
    Ok(())
}
