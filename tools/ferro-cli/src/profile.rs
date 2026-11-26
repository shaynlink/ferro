//! Performance, CPU sampling, memory allocation, Cargo build, and Mach-O binary profiling.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::Instant;

use clap::Subcommand;
use ferro_devtools::{
    ProfileSummary, parse_macos_sample_to_folded, render_svg_flamegraph, top_hot_frames,
};
use ferro_sdk::Target;

use crate::http_client::{http_get_text, resolve_devtools_url};
use crate::monitor::format_bytes;
use crate::{
    BuildOptions, build_project, cargo_target_directory, load_macos_project, safe_bundle_name,
};

#[derive(Debug, Subcommand)]
pub(crate) enum ProfileCommand {
    /// Sample CPU call stacks for a running process and emit hot frames / SVG flamegraphs.
    Cpu {
        /// Running process ID to sample (uses macOS `/usr/bin/sample`).
        #[arg(long)]
        pid: Option<u32>,
        /// Live `DevTools` endpoint URL when sampling via in-process profiler spans.
        #[arg(long)]
        url: Option<String>,
        /// Sampling duration in seconds when sampling a PID.
        #[arg(long, default_value_t = 3)]
        duration: u32,
        /// Optional path to write an SVG flamegraph.
        #[arg(long)]
        flamegraph: Option<PathBuf>,
        /// Optional path to write folded stack counts.
        #[arg(long)]
        folded: Option<PathBuf>,
        /// Emit JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Inspect live process memory regions, heap summary, and optional leak scan.
    Memory {
        /// Running process ID.
        #[arg(long)]
        pid: u32,
        /// Run macOS `leaks` check in addition to `vmmap` and `heap`.
        #[arg(long)]
        leaks: bool,
    },
    /// Measure Cargo compilation timings across the project workspace.
    Build {
        /// Project directory containing ferro.toml or Cargo.toml.
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Profile `--release` build instead of debug.
        #[arg(long)]
        release: bool,
        /// Emit JSON timing summary.
        #[arg(long)]
        json: bool,
    },
    /// Inspect Mach-O sections, top symbols by size, and `.app` bundle disk footprint.
    Binary {
        /// Project directory containing ferro.toml.
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Inspect release bundle instead of debug.
        #[arg(long)]
        release: bool,
        /// Number of largest symbols to print.
        #[arg(long, default_value_t = 15)]
        top: usize,
    },
    /// Export Chrome Trace (`chrome://tracing` / Perfetto) or SVG flamegraph from live `DevTools`.
    Trace {
        /// Explicit loopback `DevTools` URL.
        #[arg(long)]
        url: Option<String>,
        /// Running Ferro process PID.
        #[arg(long)]
        pid: Option<u32>,
        /// Output path for Chrome Trace JSON.
        #[arg(long)]
        chrome_trace: Option<PathBuf>,
        /// Output path for SVG flamegraph.
        #[arg(long)]
        flamegraph: Option<PathBuf>,
        /// Emit profiler summary JSON.
        #[arg(long)]
        json: bool,
    },
}

pub(crate) fn run(command: ProfileCommand) -> Result<(), String> {
    match command {
        ProfileCommand::Cpu {
            pid,
            url,
            duration,
            flamegraph,
            folded,
            json,
        } => profile_cpu(
            pid,
            url.as_deref(),
            duration,
            flamegraph.as_deref(),
            folded.as_deref(),
            json,
        ),
        ProfileCommand::Memory { pid, leaks } => profile_memory(pid, leaks),
        ProfileCommand::Build {
            project,
            release,
            json,
        } => profile_build(&project, release, json),
        ProfileCommand::Binary {
            project,
            release,
            top,
        } => profile_binary(&project, release, top),
        ProfileCommand::Trace {
            url,
            pid,
            chrome_trace,
            flamegraph,
            json,
        } => profile_live_trace(
            url.as_deref(),
            pid,
            chrome_trace.as_deref(),
            flamegraph.as_deref(),
            json,
        ),
    }
}

fn profile_cpu(
    pid: Option<u32>,
    url: Option<&str>,
    duration: u32,
    flamegraph_out: Option<&Path>,
    folded_out: Option<&Path>,
    json: bool,
) -> Result<(), String> {
    if let Some(pid) = pid {
        if !cfg!(target_os = "macos") {
            return Err("PID CPU sampling requires macOS `sample`".to_owned());
        }
        let output = ProcessCommand::new("sample")
            .args([&pid.to_string(), &duration.to_string(), "1", "-mayDie"])
            .output()
            .map_err(|error| format!("could not run macOS `sample`: {error}"))?;
        let text = String::from_utf8_lossy(&output.stdout);
        let folded = parse_macos_sample_to_folded(&text);
        if folded.is_empty() && !output.status.success() {
            return Err(format!(
                "sample failed for pid {pid}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        if let Some(path) = folded_out {
            let mut body = String::new();
            for (stack, count) in &folded {
                let _ = writeln!(body, "{stack} {count}");
            }
            fs::write(path, body)
                .map_err(|error| format!("could not write {}: {error}", path.display()))?;
        }
        if let Some(path) = flamegraph_out {
            let svg = render_svg_flamegraph(&format!("CPU Profile PID {pid}"), &folded);
            fs::write(path, svg)
                .map_err(|error| format!("could not write {}: {error}", path.display()))?;
        }
        let hot = top_hot_frames(&folded, 20);
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "pid": pid,
                    "duration_secs": duration,
                    "hot_frames": hot,
                }))
                .map_err(|error| error.to_string())?
            );
        } else {
            println!("Top hot frames for PID {pid} ({duration}s):");
            for (frame, samples) in hot {
                println!("  {samples:>6}  {frame}");
            }
        }
        return Ok(());
    }

    profile_live_trace(url, None, None, flamegraph_out, json)
}

fn profile_memory(pid: u32, check_leaks: bool) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("process memory profiling via vmmap/heap/leaks requires macOS".to_owned());
    }
    println!("=== vmmap -summary (pid {pid}) ===");
    let vmmap = ProcessCommand::new("vmmap")
        .args(["-summary", &pid.to_string()])
        .output()
        .map_err(|error| format!("could not run vmmap: {error}"))?;
    println!("{}", String::from_utf8_lossy(&vmmap.stdout));

    println!("=== heap -s (pid {pid}) ===");
    let heap = ProcessCommand::new("heap")
        .args(["-s", &pid.to_string()])
        .output()
        .map_err(|error| format!("could not run heap: {error}"))?;
    for line in String::from_utf8_lossy(&heap.stdout).lines().take(30) {
        println!("{line}");
    }

    if check_leaks {
        println!("=== leaks (pid {pid}) ===");
        let leaks = ProcessCommand::new("leaks")
            .arg(pid.to_string())
            .output()
            .map_err(|error| format!("could not run leaks: {error}"))?;
        for line in String::from_utf8_lossy(&leaks.stdout).lines().take(40) {
            println!("{line}");
        }
    }
    Ok(())
}

fn profile_build(project: &Path, release: bool, json: bool) -> Result<(), String> {
    let start = Instant::now();
    let mut cmd = ProcessCommand::new("cargo");
    cmd.args(["build", "--workspace", "--timings=html"]);
    if release {
        cmd.arg("--release");
    }
    let status = cmd
        .current_dir(project)
        .status()
        .map_err(|error| format!("could not execute cargo build: {error}"))?;
    let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let timings_dir = cargo_target_directory(project).join("cargo-timings");
    let report_path = timings_dir.join("cargo-timing.html");

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "success": status.success(),
                "release": release,
                "duration_ms": elapsed_ms,
                "timings_html": report_path.exists().then(|| report_path.display().to_string()),
            }))
            .map_err(|error| error.to_string())?
        );
    } else {
        println!(
            "Cargo build completed in {elapsed_ms} ms (success={})",
            status.success()
        );
        if report_path.is_file() {
            println!("Cargo timings report: {}", report_path.display());
        }
    }
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo build failed ({status})"))
    }
}

fn profile_binary(project: &Path, release: bool, top: usize) -> Result<(), String> {
    let (manifest, _) = load_macos_project(project, Target::Macos)?;
    let bundle = build_project(
        project,
        Target::Macos,
        BuildOptions {
            release,
            ..BuildOptions::default()
        },
    )?;
    let inspection = ferro_devtools::inspect_macos_bundle(&bundle.path)?;
    let exe_size = inspection.executable_size_bytes.unwrap_or(0);
    println!(
        "Binary profile — {} ({})\n  Bundle size: {} | Executable: {}",
        manifest.application().name(),
        safe_bundle_name(manifest.application().name()),
        format_bytes(inspection.total_size_bytes),
        format_bytes(exe_size),
    );

    if cfg!(target_os = "macos") {
        if let Ok(output) = ProcessCommand::new("size")
            .args(["-m"])
            .arg(&bundle.executable)
            .output()
            && output.status.success()
        {
            println!("\n[Mach-O Segments]");
            for line in String::from_utf8_lossy(&output.stdout).lines().take(25) {
                println!("  {line}");
            }
        }
        if let Ok(output) = ProcessCommand::new("nm")
            .args(["--size-sort", "-r"])
            .arg(&bundle.executable)
            .output()
            && output.status.success()
        {
            println!("\n[Top {top} Largest Symbols]");
            for line in String::from_utf8_lossy(&output.stdout).lines().take(top) {
                println!("  {line}");
            }
        }
    }
    Ok(())
}

fn profile_live_trace(
    url: Option<&str>,
    pid: Option<u32>,
    chrome_trace_out: Option<&Path>,
    flamegraph_out: Option<&Path>,
    json: bool,
) -> Result<(), String> {
    let base = resolve_devtools_url(url, pid)?;
    if let Some(path) = chrome_trace_out {
        let body = http_get_text(&base, "/profile/trace.json")?;
        fs::write(path, body)
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
        println!("Wrote Chrome trace to {}", path.display());
    }
    if let Some(path) = flamegraph_out {
        let svg = http_get_text(&base, "/profile/flamegraph.svg")?;
        fs::write(path, svg)
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
        println!("Wrote SVG flamegraph to {}", path.display());
    }
    let summary_raw = http_get_text(&base, "/profile")?;
    if json {
        println!("{summary_raw}");
    } else {
        let summary: ProfileSummary = serde_json::from_str(&summary_raw)
            .map_err(|error| format!("invalid DevTools profile payload: {error}"))?;
        println!(
            "Live Profiler — {} frames ({} dropped > {} us, avg {:.1} us, {:.1} FPS)",
            summary.frames.total_frames,
            summary.frames.dropped_frames,
            summary.frames.budget_us,
            summary.frames.avg_us,
            summary.frames.estimated_fps
        );
        for span in summary.spans {
            println!(
                "  {:<20} [{}] x{:<4} total {} us  avg {:.1} us  p95 {} us  max {} us",
                span.name,
                span.category,
                span.count,
                span.total_us,
                span.avg_us,
                span.p95_us,
                span.max_us
            );
        }
    }
    Ok(())
}
