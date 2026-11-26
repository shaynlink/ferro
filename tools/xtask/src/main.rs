//! Repository CI, smoke-testing, and policy automation (`cargo run -p xtask -- <cmd>`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "xtask", about = "Ferro repository automation and CI runner")]
struct Cli {
    #[command(subcommand)]
    command: XtaskCommand,
}

#[derive(Debug, Subcommand)]
enum XtaskCommand {
    /// Run the full repository CI matrix (`fmt`, `clippy`, `test`, `doc`, `audit`, `smoke`).
    Ci {
        /// Skip the end-to-end generated-project smoke suite.
        #[arg(long)]
        skip_smoke: bool,
    },
    /// Run fast lint and unit test gates (`fmt`, `clippy`, `test`).
    Check,
    /// Run end-to-end CLI, UI automation, telemetry, and bundle smoke tests.
    Smoke,
    /// Audit workspace crate policies (`unsafe_code`, `missing_docs`, workspace lints).
    Audit,
}

fn main() {
    let cli = Cli::parse();
    let root = workspace_root();
    let result = match cli.command {
        XtaskCommand::Ci { skip_smoke } => run_ci(&root, skip_smoke),
        XtaskCommand::Check => run_check(&root),
        XtaskCommand::Smoke => run_smoke(&root),
        XtaskCommand::Audit => run_audit(&root),
    };
    if let Err(error) = result {
        eprintln!("xtask: {error}");
        std::process::exit(1);
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

fn run_ci(root: &Path, skip_smoke: bool) -> Result<(), String> {
    let start = Instant::now();
    run_audit(root)?;
    run_check(root)?;
    run_docs_check(root)?;
    if !skip_smoke {
        run_smoke(root)?;
    }
    println!(
        "xtask ci: all checks passed in {} ms",
        start.elapsed().as_millis()
    );
    Ok(())
}

fn run_check(root: &Path) -> Result<(), String> {
    run_cargo(root, &["fmt", "--check"], &[])?;
    if cfg!(target_os = "macos") {
        run_cargo(
            root,
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
            &[],
        )?;
        run_cargo(root, &["test", "--workspace"], &[])?;
    } else {
        run_cargo(
            root,
            &[
                "clippy",
                "--workspace",
                "--exclude",
                "ferro-macos",
                "--exclude",
                "ferro-macos-runtime",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
            &[],
        )?;
        run_cargo(
            root,
            &[
                "test",
                "--workspace",
                "--exclude",
                "ferro-macos",
                "--exclude",
                "ferro-macos-runtime",
            ],
            &[],
        )?;
    }
    Ok(())
}

fn run_docs_check(root: &Path) -> Result<(), String> {
    let args: &[&str] = if cfg!(target_os = "macos") {
        &["doc", "--no-deps", "--workspace"]
    } else {
        &[
            "doc",
            "--no-deps",
            "--workspace",
            "--exclude",
            "ferro-macos",
            "--exclude",
            "ferro-macos-runtime",
        ]
    };
    run_cargo(root, args, &[("RUSTDOCFLAGS", "-D warnings")])
}

fn run_smoke(root: &Path) -> Result<(), String> {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |dur| dur.as_millis());
    let temp_root = std::env::temp_dir().join(format!("ferro-xtask-smoke-{unique}"));
    fs::create_dir_all(&temp_root).map_err(|error| {
        format!(
            "could not create smoke dir `{}`: {error}",
            temp_root.display()
        )
    })?;

    let outcome = run_smoke_inner(root, &temp_root);
    let _ = fs::remove_dir_all(&temp_root);
    outcome
}

fn run_smoke_inner(root: &Path, temp_root: &Path) -> Result<(), String> {
    let project = temp_root.join("atlas-smoke");
    let project_str = project.display().to_string();

    run_ferro(root, &["init", &project_str])?;
    run_ferro(root, &["ci", "init", "--project", &project_str])?;

    let monitor_out = run_ferro_capture(
        root,
        &[
            "monitor",
            "--section",
            "all",
            "--interval-ms",
            "50",
            "--json",
        ],
    )?;
    for required in [
        "\"cpu\"",
        "\"memory\"",
        "\"gpu\"",
        "\"neural_engine\"",
        "\"network\"",
        "\"storage\"",
    ] {
        if !monitor_out.contains(required) {
            return Err(format!(
                "`ferro monitor --json` missing expected section {required}"
            ));
        }
    }

    let ui_tree_path = temp_root.join("ui-tree.json");
    let ui_tree_str = ui_tree_path.display().to_string();
    fs::write(
        &ui_tree_path,
        r#"{"app_name":"Atlas","window_id":"main","root":{"id":"main","class_name":"NSWindow","role":"window","title":"Atlas","value":null,"frame":{"x":0.0,"y":0.0,"width":800.0,"height":520.0},"enabled":true,"visible":true,"focused":true,"children":[{"id":"open-btn","class_name":"NSButton","role":"button","title":"Open","value":null,"frame":{"x":20.0,"y":20.0,"width":100.0,"height":32.0},"enabled":true,"visible":true,"focused":false,"children":[]}]}}"#,
    )
    .map_err(|error| format!("could not write UI tree fixture: {error}"))?;
    let junit_path = temp_root.join("ui-junit.xml");
    let junit_str = junit_path.display().to_string();
    run_ferro(
        root,
        &[
            "test",
            "ui",
            "--tree",
            &ui_tree_str,
            "--strict-a11y",
            "--junit",
            &junit_str,
        ],
    )?;
    if !junit_path.is_file() {
        return Err("`ferro test ui --junit` did not produce JUnit XML".to_owned());
    }

    if cfg!(target_os = "macos") {
        run_ferro(root, &["doctor", "--project", &project_str])?;
        run_ferro(root, &["build", "macos", "--project", &project_str])?;
        run_ferro(root, &["debug", "inspect", "--project", &project_str])?;
        run_ferro(root, &["dev", "--project", &project_str, "--once"])?;
    }

    println!("xtask smoke: all CLI, telemetry, UI automation, and bundle checks passed");
    Ok(())
}

fn run_audit(root: &Path) -> Result<(), String> {
    let issues = audit_workspace_policies(root)?;
    if issues.is_empty() {
        println!("xtask audit: workspace safety and lint policies verified");
        Ok(())
    } else {
        Err(format!("policy audit failed: {}", issues.join("; ")))
    }
}

fn audit_workspace_policies(root: &Path) -> Result<Vec<String>, String> {
    let root_manifest = fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|error| format!("could not read root Cargo.toml: {error}"))?;
    let mut issues = Vec::new();
    if !root_manifest.contains("unsafe_code = \"forbid\"") {
        issues.push("root Cargo.toml must keep `unsafe_code = \"forbid\"`".to_owned());
    }
    if !root_manifest.contains("missing_docs = \"warn\"") {
        issues.push("root Cargo.toml must keep `missing_docs = \"warn\"`".to_owned());
    }

    let allowed_unsafe = ["crates/ferro-macos-runtime", "apps/ferro-macos"];
    for group in ["crates", "apps", "tools"] {
        let dir = root.join(group);
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let cargo_path = entry.path().join("Cargo.toml");
            if !cargo_path.is_file() {
                continue;
            }
            let rel = format!("{group}/{}", entry.file_name().to_string_lossy());
            let content = fs::read_to_string(&cargo_path)
                .map_err(|error| format!("could not read `{}`: {error}", cargo_path.display()))?;
            if content.contains("unsafe_code = \"allow\"")
                && !allowed_unsafe.contains(&rel.as_str())
            {
                issues.push(format!(
                    "{rel} sets `unsafe_code = \"allow\"` outside macOS FFI crates"
                ));
            }
            if !content.contains("[lints]") && !content.contains("[lints.rust]") {
                issues.push(format!("{rel}/Cargo.toml is missing `[lints]` inheritance"));
            }
            if content.contains("[lints.rust]") && !content.contains("missing_docs = \"warn\"") {
                issues.push(format!(
                    "{rel}/Cargo.toml overrides `[lints.rust]` without `missing_docs = \"warn\"`"
                ));
            }
        }
    }
    Ok(issues)
}

fn run_cargo(root: &Path, args: &[&str], envs: &[(&str, &str)]) -> Result<(), String> {
    println!("==> cargo {}", args.join(" "));
    let mut cmd = Command::new("cargo");
    cmd.args(args).current_dir(root);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let status = cmd
        .status()
        .map_err(|error| format!("failed to run cargo {}: {error}", args.join(" ")))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo {} exited with {status}", args.join(" ")))
    }
}

fn run_ferro(root: &Path, args: &[&str]) -> Result<(), String> {
    let mut full = vec!["run", "-p", "ferro", "--"];
    full.extend_from_slice(args);
    run_cargo(root, &full, &[])
}

fn run_ferro_capture(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut full = vec!["run", "-q", "-p", "ferro", "--"];
    full.extend_from_slice(args);
    let output = Command::new("cargo")
        .args(&full)
        .current_dir(root)
        .output()
        .map_err(|error| format!("failed to run ferro {}: {error}", args.join(" ")))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!(
            "ferro {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{audit_workspace_policies, workspace_root};

    #[test]
    fn workspace_passes_policy_audit() {
        let issues = audit_workspace_policies(&workspace_root()).expect("audit workspace");
        assert_eq!(issues, Vec::<String>::new());
    }
}
