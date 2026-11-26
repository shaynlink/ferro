//! Unit, UI automation, and accessibility test runner (`ferro test`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command as ProcessCommand};
use std::thread;
use std::time::{Duration, Instant};

use clap::Subcommand;
use ferro_sdk::Target;
use ferro_testkit::{
    AutomationReport, AutomationScenario, UiAction, UiSelector, UiTree, evaluate_scenario_on_tree,
};
use serde::Serialize;

use crate::http_client::{discover_devtools_targets, http_get_text, http_post_json};
use crate::{BuildOptions, build_project};

#[derive(Debug, Subcommand)]
pub(crate) enum TestCommand {
    /// Run Rust unit and integration tests for the project.
    Unit {
        /// Project directory containing Cargo.toml.
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Use `cargo-nextest` if available.
        #[arg(long)]
        nextest: bool,
        /// Optional path to write a `JUnit` XML report.
        #[arg(long)]
        junit: Option<PathBuf>,
        /// Emit JSON summary.
        #[arg(long)]
        json: bool,
    },
    /// Run UI automation and accessibility checks against a UI tree fixture or live macOS app.
    Ui {
        /// Project directory containing ferro.toml.
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Path to a JSON `AutomationScenario` file (uses a default smoke scenario if omitted).
        #[arg(long)]
        scenario: Option<PathBuf>,
        /// Path to an offline `UiTree` JSON snapshot (enables cross-platform UI automation testing).
        #[arg(long)]
        tree: Option<PathBuf>,
        /// Build and launch the macOS app with `DevTools` to execute the scenario against live `AppKit` views.
        #[arg(long)]
        launch: bool,
        /// Fail on accessibility warnings in addition to errors.
        #[arg(long)]
        strict_a11y: bool,
        /// Optional directory to capture a macOS window screenshot during `--launch`.
        #[arg(long)]
        screenshot_dir: Option<PathBuf>,
        /// Optional path to write a `JUnit` XML report.
        #[arg(long)]
        junit: Option<PathBuf>,
        /// Emit JSON report.
        #[arg(long)]
        json: bool,
    },
    /// Run unit tests plus manifest and bundle verification.
    All {
        /// Project directory containing ferro.toml.
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Emit JSON report.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Serialize)]
pub(crate) struct UnitTestSummary {
    pub(crate) runner: String,
    pub(crate) passed: bool,
    pub(crate) duration_ms: u64,
    pub(crate) test_cases_passed: usize,
    pub(crate) test_cases_failed: usize,
    pub(crate) test_cases_ignored: usize,
}

pub(crate) fn run(command: TestCommand) -> Result<(), String> {
    match command {
        TestCommand::Unit {
            project,
            nextest,
            junit,
            json,
        } => {
            let summary = run_unit_tests(&project, nextest)?;
            if let Some(junit_path) = junit {
                let xml = render_unit_junit(&summary);
                fs::write(&junit_path, xml).map_err(|error| {
                    format!("could not write {}: {error}", junit_path.display())
                })?;
            }
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&summary).map_err(|error| error.to_string())?
                );
            } else {
                println!(
                    "Unit tests ({}) completed in {} ms: {} passed, {} failed, {} ignored",
                    summary.runner,
                    summary.duration_ms,
                    summary.test_cases_passed,
                    summary.test_cases_failed,
                    summary.test_cases_ignored
                );
            }
            if summary.passed {
                Ok(())
            } else {
                Err("unit test suite failed".to_owned())
            }
        }
        TestCommand::Ui {
            project,
            scenario,
            tree,
            launch,
            strict_a11y,
            screenshot_dir,
            junit,
            json,
        } => run_ui_tests(
            &project,
            scenario.as_deref(),
            tree.as_deref(),
            launch,
            strict_a11y,
            screenshot_dir.as_deref(),
            junit.as_deref(),
            json,
        ),
        TestCommand::All { project, json } => run_all_tests(&project, json),
    }
}

pub(crate) fn run_unit_tests(project: &Path, use_nextest: bool) -> Result<UnitTestSummary, String> {
    let start = Instant::now();
    let has_nextest = use_nextest
        && ProcessCommand::new("cargo")
            .args(["nextest", "--version"])
            .output()
            .is_ok_and(|out| out.status.success());

    let (runner, args): (&str, &[&str]) = if has_nextest {
        ("cargo-nextest", &["nextest", "run", "--workspace"])
    } else {
        ("cargo-test", &["test", "--workspace"])
    };

    let output = ProcessCommand::new("cargo")
        .args(args)
        .current_dir(project)
        .output()
        .map_err(|error| format!("could not execute {runner}: {error}"))?;
    let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let (passed_count, failed_count, ignored_count) = parse_cargo_test_totals(&combined);
    if !output.status.success() {
        eprint!("{combined}");
    }
    Ok(UnitTestSummary {
        runner: runner.to_owned(),
        passed: output.status.success(),
        duration_ms,
        test_cases_passed: passed_count,
        test_cases_failed: failed_count,
        test_cases_ignored: ignored_count,
    })
}

fn parse_cargo_test_totals(output: &str) -> (usize, usize, usize) {
    let mut passed = 0_usize;
    let mut failed = 0_usize;
    let mut ignored = 0_usize;
    for line in output.lines() {
        if line.contains("test result:") {
            for part in line.split(';') {
                let mut tokens = part.split_whitespace().collect::<Vec<_>>();
                tokens.reverse();
                if let (Some(label), Some(num_str)) = (tokens.first(), tokens.get(1))
                    && let Ok(count) = num_str.parse::<usize>()
                {
                    match *label {
                        "passed" => passed += count,
                        "failed" => failed += count,
                        "ignored" => ignored += count,
                        _ => {}
                    }
                }
            }
        }
    }
    (passed, failed, ignored)
}

fn render_unit_junit(summary: &UnitTestSummary) -> String {
    #[allow(
        clippy::cast_precision_loss,
        reason = "millisecond durations fit cleanly in f64 seconds for JUnit"
    )]
    let secs = (summary.duration_ms as f64) / 1000.0;
    let failure_tag = if summary.passed {
        " />\n".to_owned()
    } else {
        format!(
            ">\n    <failure message=\"{} test(s) failed\" />\n  </testcase>\n",
            summary.test_cases_failed
        )
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuite name=\"ferro-unit\" tests=\"{}\" failures=\"{}\" time=\"{secs:.3}\">\n  <testcase name=\"{}\" time=\"{secs:.3}\"{failure_tag}</testsuite>\n",
        summary.test_cases_passed + summary.test_cases_failed,
        summary.test_cases_failed,
        summary.runner
    )
}

#[allow(clippy::too_many_arguments)]
fn run_ui_tests(
    project: &Path,
    scenario_path: Option<&Path>,
    tree_path: Option<&Path>,
    launch: bool,
    strict_a11y: bool,
    screenshot_dir: Option<&Path>,
    junit_path: Option<&Path>,
    json: bool,
) -> Result<(), String> {
    let scenario = match scenario_path {
        Some(path) => {
            let raw = fs::read_to_string(path).map_err(|error| {
                format!("could not read scenario `{}`: {error}", path.display())
            })?;
            serde_json::from_str::<AutomationScenario>(&raw)
                .map_err(|error| format!("invalid scenario JSON `{}`: {error}", path.display()))?
        }
        None => default_smoke_scenario(strict_a11y),
    };

    let report = if let Some(tree_file) = tree_path {
        let raw = fs::read_to_string(tree_file).map_err(|error| {
            format!("could not read UI tree `{}`: {error}", tree_file.display())
        })?;
        let mut tree = serde_json::from_str::<UiTree>(&raw)
            .map_err(|error| format!("invalid UI tree JSON `{}`: {error}", tree_file.display()))?;
        evaluate_scenario_on_tree(&scenario, &mut tree, |action, _| {
            Err(format!(
                "offline UI tree mode cannot execute live mutation `{action:?}`"
            ))
        })
    } else if launch {
        run_live_macos_ui_scenario(project, &scenario, screenshot_dir)?
    } else {
        return Err("specify either `--tree <ui_tree.json>` or `--launch`".to_owned());
    };

    if let Some(junit) = junit_path {
        fs::write(junit, report.to_junit_xml())
            .map_err(|error| format!("could not write {}: {error}", junit.display()))?;
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?
        );
    } else {
        println!(
            "UI Automation `{}` — {} ({} steps, {} ms, {} a11y issue(s))",
            report.scenario,
            if report.passed { "PASSED" } else { "FAILED" },
            report.steps.len(),
            report.duration_ms,
            report.accessibility_issues.len()
        );
        for step in &report.steps {
            println!(
                "  [{}] step {}: {} ({} ms){}",
                if step.passed { "PASS" } else { "FAIL" },
                step.index,
                step.description,
                step.duration_ms,
                step.detail
                    .as_deref()
                    .map_or_else(String::new, |msg| format!(" — {msg}"))
            );
        }
    }

    if report.passed {
        Ok(())
    } else {
        Err(format!(
            "UI automation scenario `{}` failed",
            report.scenario
        ))
    }
}

fn default_smoke_scenario(strict_a11y: bool) -> AutomationScenario {
    AutomationScenario {
        name: "ferro-ui-smoke".to_owned(),
        steps: vec![
            UiAction::AssertExists {
                selector: UiSelector::Id("main".to_owned()),
            },
            UiAction::AssertVisible {
                selector: UiSelector::Id("main".to_owned()),
            },
            UiAction::AuditAccessibility {
                strict: strict_a11y,
            },
        ],
    }
}

fn run_live_macos_ui_scenario(
    project: &Path,
    scenario: &AutomationScenario,
    screenshot_dir: Option<&Path>,
) -> Result<AutomationReport, String> {
    let bundle = build_project(project, Target::Macos, BuildOptions::default())?;
    let mut child = ProcessCommand::new(&bundle.executable)
        .current_dir(project)
        .env("FERRO_DEVTOOLS", "1")
        .spawn()
        .map_err(|error| format!("could not launch {}: {error}", bundle.path.display()))?;

    let result = execute_against_running_app(&mut child, scenario, screenshot_dir);
    let _ = child.kill();
    let _ = child.wait();
    result
}

fn execute_against_running_app(
    child: &mut Child,
    scenario: &AutomationScenario,
    screenshot_dir: Option<&Path>,
) -> Result<AutomationReport, String> {
    let pid = child.id();
    let mut endpoint_url = None;
    for _ in 0..50 {
        if let Some(found) = discover_devtools_targets()
            .into_iter()
            .find(|item| item.pid == pid)
        {
            endpoint_url = Some(found.url);
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let url = endpoint_url.ok_or_else(|| {
        "launched app did not publish a DevTools UI endpoint within 5s".to_owned()
    })?;

    let mut tree = fetch_live_ui_tree(&url)?;
    let report = evaluate_scenario_on_tree(scenario, &mut tree, |action, current_tree| {
        let payload = serde_json::to_string(action).map_err(|error| error.to_string())?;
        http_post_json(&url, "/ui/action", &payload)?;
        thread::sleep(Duration::from_millis(120));
        if let Ok(updated) = fetch_live_ui_tree(&url) {
            *current_tree = updated;
        }
        Ok("dispatched to live AppKit window".to_owned())
    });

    if let Some(dir) = screenshot_dir
        && cfg!(target_os = "macos")
    {
        let _ = fs::create_dir_all(dir);
        let shot = dir.join(format!("{}.png", scenario.name));
        let _ = ProcessCommand::new("screencapture")
            .args(["-x"])
            .arg(&shot)
            .status();
    }

    Ok(report)
}

fn fetch_live_ui_tree(base_url: &str) -> Result<UiTree, String> {
    let raw = http_get_text(base_url, "/ui")?;
    serde_json::from_str::<UiTree>(&raw)
        .map_err(|error| format!("could not decode live UI tree: {error}"))
}

fn run_all_tests(project: &Path, json: bool) -> Result<(), String> {
    let unit = run_unit_tests(project, false)?;
    let manifest = ferro_project::ProjectManifest::load(project.join("ferro.toml"))
        .map_err(|error| error.to_string())?;
    let mut bundle_built = false;
    if manifest.targets().contains(&Target::Macos) && std::env::consts::OS == "macos" {
        let _ = build_project(project, Target::Macos, BuildOptions::default())?;
        bundle_built = true;
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "unit": unit,
                "manifest_valid": true,
                "macos_bundle_verified": bundle_built,
            }))
            .map_err(|error| error.to_string())?
        );
    } else {
        println!(
            "All checks — unit: {} ({} passed), manifest: ok, macOS bundle verified: {}",
            if unit.passed { "ok" } else { "failed" },
            unit.test_cases_passed,
            bundle_built
        );
    }
    if unit.passed {
        Ok(())
    } else {
        Err("ferro test all failed".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::parse_cargo_test_totals;

    #[test]
    fn parses_cargo_test_summary_lines() {
        let out = "\
test result: ok. 12 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out
test result: FAILED. 3 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out
";
        assert_eq!(parse_cargo_test_totals(out), (15, 2, 1));
    }
}
