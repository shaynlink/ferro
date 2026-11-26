//! CI tooling for Ferro application projects (`ferro ci check` and `ferro ci init`).

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::Instant;

use clap::Subcommand;
use ferro_devtools::inspect_macos_bundle;
use ferro_project::ProjectManifest;
use ferro_sdk::Target;
use serde::Serialize;

use crate::{BuildOptions, build_project};

#[derive(Debug, Subcommand)]
pub(crate) enum CiCommand {
    /// Run full project CI gates (manifest, fmt, clippy, tests, and macOS bundle verification).
    Check {
        /// Project directory containing ferro.toml.
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Emit GitHub Actions workflow annotations (`::error::`, `::notice::`).
        #[arg(long)]
        github_annotations: bool,
        /// Write a Markdown summary report (e.g. `$GITHUB_STEP_SUMMARY`).
        #[arg(long)]
        summary_file: Option<PathBuf>,
        /// Emit JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Generate a ready-to-use `.github/workflows/ferro-ci.yml` pipeline for a Ferro project.
    Init {
        /// Project directory containing ferro.toml.
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Overwrite an existing workflow file.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Serialize)]
struct CiStepReport {
    name: String,
    passed: bool,
    skipped: bool,
    duration_ms: u64,
    detail: String,
}

#[derive(Debug, Serialize)]
struct CiReport {
    project: String,
    passed: bool,
    total_duration_ms: u64,
    steps: Vec<CiStepReport>,
}

pub(crate) fn run(command: CiCommand) -> Result<(), String> {
    match command {
        CiCommand::Check {
            project,
            github_annotations,
            summary_file,
            json,
        } => run_ci_check(&project, github_annotations, summary_file.as_deref(), json),
        CiCommand::Init { project, force } => init_ci_workflow(&project, force),
    }
}

fn run_ci_check(
    project: &Path,
    github_annotations: bool,
    summary_file: Option<&Path>,
    json: bool,
) -> Result<(), String> {
    let total_start = Instant::now();
    let mut steps = Vec::new();

    let manifest_step = run_step("manifest", || {
        let manifest =
            ProjectManifest::load(project.join("ferro.toml")).map_err(|error| error.to_string())?;
        Ok(format!(
            "{} ({}) schema {}",
            manifest.application().name(),
            manifest.application().id(),
            manifest.schema()
        ))
    });
    let manifest_ok = manifest_step.passed;
    steps.push(manifest_step);

    steps.push(run_cargo_step("fmt", project, &["fmt", "--check"]));
    steps.push(run_cargo_step(
        "clippy",
        project,
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    ));
    steps.push(run_cargo_step("test", project, &["test", "--workspace"]));

    if manifest_ok
        && let Ok(manifest) = ProjectManifest::load(project.join("ferro.toml"))
        && manifest.targets().contains(&Target::Macos)
    {
        if std::env::consts::OS == "macos" {
            steps.push(run_step("macos-bundle", || {
                let bundle = build_project(project, Target::Macos, BuildOptions::default())?;
                let info = inspect_macos_bundle(&bundle.path)?;
                Ok(format!(
                    "{} ({} B, archs: {:?})",
                    bundle.path.display(),
                    info.total_size_bytes,
                    info.architectures
                ))
            }));
        } else {
            steps.push(CiStepReport {
                name: "macos-bundle".to_owned(),
                passed: true,
                skipped: true,
                duration_ms: 0,
                detail: "skipped on non-macOS runner".to_owned(),
            });
        }
    }

    let passed = steps.iter().all(|step| step.passed);
    let report = CiReport {
        project: project.display().to_string(),
        passed,
        total_duration_ms: u64::try_from(total_start.elapsed().as_millis()).unwrap_or(u64::MAX),
        steps,
    };

    emit_ci_report(&report, github_annotations, summary_file, json)?;

    if report.passed {
        Ok(())
    } else {
        Err("ferro ci check failed".to_owned())
    }
}

fn emit_ci_report(
    report: &CiReport,
    github_annotations: bool,
    summary_file: Option<&Path>,
    json: bool,
) -> Result<(), String> {
    if github_annotations {
        for step in &report.steps {
            if !step.passed {
                println!("::error title=Ferro CI ({})::{}", step.name, step.detail);
            } else if !step.skipped {
                println!("::notice title=Ferro CI ({})::{}", step.name, step.detail);
            }
        }
    }

    if let Some(summary_path) = summary_file {
        let md = render_markdown_summary(report);
        if let Some(parent) = summary_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(summary_path, md).map_err(|error| {
            format!(
                "could not write summary `{}`: {error}",
                summary_path.display()
            )
        })?;
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(report).map_err(|error| error.to_string())?
        );
    } else {
        println!(
            "Ferro CI Check — {} ({} ms)",
            if report.passed { "PASSED" } else { "FAILED" },
            report.total_duration_ms
        );
        for step in &report.steps {
            let badge = if step.skipped {
                "SKIP"
            } else if step.passed {
                "PASS"
            } else {
                "FAIL"
            };
            println!(
                "  [{badge}] {:<14} {:>6} ms  {}",
                step.name, step.duration_ms, step.detail
            );
        }
    }
    Ok(())
}

fn run_step(name: &str, f: impl FnOnce() -> Result<String, String>) -> CiStepReport {
    let start = Instant::now();
    match f() {
        Ok(detail) => CiStepReport {
            name: name.to_owned(),
            passed: true,
            skipped: false,
            duration_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
            detail,
        },
        Err(detail) => CiStepReport {
            name: name.to_owned(),
            passed: false,
            skipped: false,
            duration_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
            detail,
        },
    }
}

fn run_cargo_step(name: &str, project: &Path, args: &[&str]) -> CiStepReport {
    run_step(name, || {
        let output = ProcessCommand::new("cargo")
            .args(args)
            .current_dir(project)
            .output()
            .map_err(|error| format!("could not start cargo {}: {error}", args.join(" ")))?;
        if output.status.success() {
            Ok(format!("cargo {} succeeded", args.join(" ")))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let excerpt = stderr.lines().take(6).collect::<Vec<_>>().join(" | ");
            Err(format!("cargo {} failed: {excerpt}", args.join(" ")))
        }
    })
}

fn render_markdown_summary(report: &CiReport) -> String {
    let mut md = format!(
        "## Ferro CI Summary ({})\n\nTotal duration: {} ms\n\n| Step | Status | Duration (ms) | Detail |\n|---|---|---:|---|\n",
        if report.passed { "PASSED" } else { "FAILED" },
        report.total_duration_ms
    );
    for step in &report.steps {
        let badge = if step.skipped {
            "SKIPPED"
        } else if step.passed {
            "PASSED"
        } else {
            "FAILED"
        };
        let _ = writeln!(
            md,
            "| `{}` | {} | {} | {} |",
            step.name,
            badge,
            step.duration_ms,
            step.detail.replace('|', "\\|")
        );
    }
    md
}

fn init_ci_workflow(project: &Path, force: bool) -> Result<(), String> {
    let manifest =
        ProjectManifest::load(project.join("ferro.toml")).map_err(|error| error.to_string())?;
    let workflow_path = project.join(".github/workflows/ferro-ci.yml");
    if workflow_path.exists() && !force {
        return Err(format!(
            "`{}` already exists (pass --force to overwrite)",
            workflow_path.display()
        ));
    }
    let content = render_project_ci_workflow(&manifest);
    if let Some(parent) = workflow_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create `{}`: {error}", parent.display()))?;
    }
    fs::write(&workflow_path, content)
        .map_err(|error| format!("could not write `{}`: {error}", workflow_path.display()))?;
    println!("Created Ferro CI workflow at {}", workflow_path.display());
    Ok(())
}

pub(crate) fn render_project_ci_workflow(manifest: &ProjectManifest) -> String {
    let mut runners = Vec::new();
    if manifest.targets().contains(&Target::Macos) {
        runners.push("macos-latest");
    }
    if manifest.targets().contains(&Target::Linux) {
        runners.push("ubuntu-latest");
    }
    if manifest.targets().contains(&Target::Windows) {
        runners.push("windows-latest");
    }
    if runners.is_empty() {
        runners.push("macos-latest");
    }
    let os_list = runners.join(", ");
    let name = manifest.application().name();
    format!(
        "name: {name} Ferro CI\n\non:\n  pull_request:\n  push:\n    branches: [main]\n\npermissions:\n  contents: read\n\njobs:\n  verify:\n    name: ${{{{ matrix.os }}}}\n    runs-on: ${{{{ matrix.os }}}}\n    strategy:\n      fail-fast: false\n      matrix:\n        os: [{os_list}]\n    steps:\n      - uses: actions/checkout@v5\n      - uses: dtolnay/rust-toolchain@1.100.0\n        with:\n          components: rustfmt, clippy\n      - run: cargo fmt --check\n      - run: cargo clippy --workspace --all-targets -- -D warnings\n      - run: cargo test --workspace\n"
    )
}

#[cfg(test)]
mod tests {
    use ferro_project::ProjectManifest;
    use ferro_sdk::Target;

    use super::render_project_ci_workflow;

    #[test]
    fn renders_workflow_for_declared_targets() {
        let manifest = ProjectManifest::new(
            "Atlas",
            "dev.ferro.atlas",
            vec![Target::Macos, Target::Linux],
        )
        .expect("manifest");
        let yaml = render_project_ci_workflow(&manifest);
        assert!(yaml.contains("os: [macos-latest, ubuntu-latest]"));
        assert!(yaml.contains("cargo clippy --workspace"));
    }
}
