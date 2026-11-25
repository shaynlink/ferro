//! The Ferro developer command-line interface.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use clap::{Parser, Subcommand};
use ferro_contracts::WorkspaceMaintenance;
use ferro_project::{ProjectError, ProjectManifest};
use ferro_sdk::Target;
use ferro_storage::WorkspaceDatabase;

/// Ferro's developer command line.
#[derive(Debug, Parser)]
#[command(
    name = "ferro",
    version,
    about = "Project tooling for Ferro applications"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a desktop Ferro application project.
    Init {
        /// Directory to create. It must not already exist.
        path: PathBuf,
        /// Stable reverse-DNS application identifier.
        #[arg(long)]
        id: Option<String>,
        /// Comma-separated desktop targets.
        #[arg(
            long,
            value_delimiter = ',',
            default_values = ["macos", "windows", "linux"]
        )]
        targets: Vec<Target>,
    },
    /// Inspect the local toolchain and an optional Ferro project.
    Doctor {
        /// Project directory containing ferro.toml.
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Compile and launch a supported native development target.
    Run {
        /// Project directory containing ferro.toml.
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Declared target to launch.
        #[arg(long, default_value = "macos")]
        target: Target,
    },
    /// Inspect Ferro target support.
    Target {
        #[command(subcommand)]
        command: TargetCommand,
    },
    /// Verify an internal experimental `LiliaDB` database.
    Storage {
        /// Path to a `LiliaDB` database directory.
        path: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum TargetCommand {
    /// List targets recognised by this version of Ferro.
    List,
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Init { path, id, targets } => init_project(&path, id, targets),
        Command::Doctor { project } => doctor(&project),
        Command::Run { project, target } => run_project(&project, target),
        Command::Target {
            command: TargetCommand::List,
        } => {
            list_targets();
            Ok(())
        }
        Command::Storage { path } => storage(&path),
    };
    if let Err(error) = result {
        eprintln!("ferro: {error}");
        std::process::exit(1);
    }
}

fn init_project(path: &Path, id: Option<String>, targets: Vec<Target>) -> Result<(), String> {
    if path.exists() {
        return Err(format!(
            "refusing to create a project in existing path `{}`",
            path.display()
        ));
    }
    let slug = project_slug(path)?;
    let id = id.unwrap_or_else(|| format!("dev.ferro.{slug}"));
    let manifest = ProjectManifest::new(&slug, id, targets).map_err(|error| error.to_string())?;

    fs::create_dir_all(path).map_err(|error| {
        format!(
            "could not create project directory `{}`: {error}",
            path.display()
        )
    })?;
    write_file(&path.join("ferro.toml"), &manifest.to_toml())?;
    write_file(
        &path.join("Cargo.toml"),
        &render_workspace_manifest(&slug, manifest.targets()),
    )?;
    write_file(
        &path.join("README.md"),
        &render_project_readme(&slug, manifest.application().id(), manifest.targets()),
    )?;
    write_file(
        &path
            .join("crates")
            .join(format!("{slug}-core"))
            .join("Cargo.toml"),
        &render_core_manifest(&slug),
    )?;
    write_file(
        &path
            .join("crates")
            .join(format!("{slug}-core"))
            .join("src/lib.rs"),
        &render_core_source(&slug, manifest.application().id()),
    )?;
    for target in manifest.targets() {
        let host = path.join("apps").join(format!("{slug}-{target}"));
        write_file(
            &host.join("Cargo.toml"),
            &render_host_manifest(&slug, *target),
        )?;
        write_file(
            &host.join("src/main.rs"),
            &render_host_source(&slug, *target),
        )?;
    }

    println!(
        "Created Ferro project `{}` at {}",
        manifest.application().name(),
        path.display()
    );
    println!("Declared targets: {}", join_targets(manifest.targets()));
    println!(
        "Next: ferro run --project {} --target macos",
        path.display()
    );
    Ok(())
}

fn doctor(project: &Path) -> Result<(), String> {
    println!("Ferro doctor");
    println!("- Host OS: {}", std::env::consts::OS);
    match ProcessCommand::new("rustc").arg("--version").output() {
        Ok(output) if output.status.success() => {
            println!("- Rust: {}", String::from_utf8_lossy(&output.stdout).trim());
        }
        Ok(_) => println!("- Rust: unavailable (rustc --version failed)"),
        Err(error) => println!("- Rust: unavailable ({error})"),
    }

    let manifest_path = project.join("ferro.toml");
    match ProjectManifest::load(&manifest_path) {
        Ok(manifest) => {
            println!(
                "- Project: {} ({})",
                manifest.application().name(),
                manifest.application().id()
            );
            for target in manifest.targets() {
                println!("- Target {target}: {}", target_status(*target));
            }
        }
        Err(ProjectError::Read { error, .. }) if error.kind() == std::io::ErrorKind::NotFound => {
            println!("- Project: no ferro.toml at {}", manifest_path.display());
        }
        Err(error) => return Err(error.to_string()),
    }
    Ok(())
}

fn run_project(project: &Path, target: Target) -> Result<(), String> {
    run_project_with(project, target, |command| {
        let status = command
            .status()
            .map_err(|error| format!("could not start Cargo: {error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "Cargo failed while running the {target} target ({status})"
            ))
        }
    })
}

fn run_project_with(
    project: &Path,
    target: Target,
    execute: impl FnOnce(&mut ProcessCommand) -> Result<(), String>,
) -> Result<(), String> {
    let manifest_path = project.join("ferro.toml");
    let manifest = ProjectManifest::load(&manifest_path).map_err(|error| error.to_string())?;
    if !manifest.targets().contains(&target) {
        return Err(format!(
            "project `{}` does not declare the {target} target",
            manifest.application().name()
        ));
    }
    if target != Target::Macos {
        return Err(format!(
            "the {target} target is declared but has no Ferro development runtime yet"
        ));
    }
    if std::env::consts::OS != "macos" {
        return Err("the macos target can only run on macOS".to_owned());
    }

    let slug = project_slug(project)?;
    let package = format!("{slug}-{target}");
    let mut command = ProcessCommand::new("cargo");
    command
        .args(["run", "--package", &package])
        .current_dir(project);
    execute(&mut command)
}

fn list_targets() {
    println!("Ferro targets");
    for target in Target::all() {
        println!("- {target}: {}", target_status(target));
    }
}

fn target_status(target: Target) -> &'static str {
    match target {
        Target::Macos => "supported for local development on macOS; packaging is not yet supported",
        Target::Windows | Target::Linux => "declarable; native runtime is not implemented",
    }
}

fn storage(path: &Path) -> Result<(), String> {
    let database = WorkspaceDatabase::open(path)
        .map_err(|error| format!("could not open workspace database: {error}"))?;
    match database.integrity_check() {
        Ok(true) => {
            println!("Ferro storage: healthy");
            Ok(())
        }
        Ok(false) => Err("storage integrity check failed".to_owned()),
        Err(error) => Err(format!("could not check workspace database: {error}")),
    }
}

fn project_slug(path: &Path) -> Result<String, String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "project path must end with a UTF-8 directory name".to_owned())?;
    let slug = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        return Err("project directory must contain an ASCII letter or number".to_owned());
    }
    Ok(slug.to_owned())
}

fn write_file(path: &Path, contents: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("path `{}` has no parent directory", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create `{}`: {error}", parent.display()))?;
    fs::write(path, contents)
        .map_err(|error| format!("could not write `{}`: {error}", path.display()))
}

fn render_workspace_manifest(slug: &str, targets: &[Target]) -> String {
    let mut members = vec![format!("  \"crates/{slug}-core\",")];
    members.extend(
        targets
            .iter()
            .map(|target| format!("  \"apps/{slug}-{target}\",")),
    );
    format!(
        "[workspace]\nmembers = [\n{}\n]\nresolver = \"3\"\n\n[workspace.package]\nedition = \"2024\"\nrust-version = \"1.100\"\nversion = \"0.1.0\"\n",
        members.join("\n")
    )
}

fn render_core_manifest(slug: &str) -> String {
    format!(
        "[package]\nname = \"{slug}-core\"\nversion.workspace = true\nedition.workspace = true\nrust-version.workspace = true\n\n[dependencies]\nferro-sdk = {{ path = \"{}\" }}\n\n[lints.rust]\nmissing_docs = \"warn\"\n",
        framework_crate_path("crates/ferro-sdk")
    )
}

fn render_core_source(slug: &str, id: &str) -> String {
    format!(
        "//! Shared application entry point for `{slug}`.\n\n/// Ferro application metadata consumed by native target runtimes.\npub struct Application;\n\nimpl ferro_sdk::Application for Application {{\n    fn descriptor() -> ferro_sdk::ApplicationDescriptor {{\n        ferro_sdk::ApplicationDescriptor::new(\"{slug}\", \"{id}\")\n    }}\n}}\n"
    )
}

fn render_host_manifest(slug: &str, target: Target) -> String {
    let runtime = if target == Target::Macos {
        format!(
            "ferro-macos-runtime = {{ path = \"{}\" }}\n",
            framework_crate_path("crates/ferro-macos-runtime")
        )
    } else {
        String::new()
    };
    format!(
        "[package]\nname = \"{slug}-{target}\"\nversion.workspace = true\nedition.workspace = true\nrust-version.workspace = true\n\n[dependencies]\n{slug}-core = {{ path = \"../../crates/{slug}-core\" }}\nferro-sdk = {{ path = \"{}\" }}\n{runtime}",
        framework_crate_path("crates/ferro-sdk")
    )
}

fn render_host_source(slug: &str, target: Target) -> String {
    if target == Target::Macos {
        return format!(
            "fn main() {{\n    ferro_macos_runtime::run::<{slug}_core::Application>();\n}}\n"
        );
    }
    format!(
        "use ferro_sdk::Application as _;\n\nfn main() {{\n    let descriptor = {slug}_core::Application::descriptor();\n    println!(\"{slug}: {target} host scaffold (runtime not implemented)\");\n    println!(\"Application id: {{}}\", descriptor.id());\n}}\n"
    )
}

fn framework_crate_path(relative: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
        .canonicalize()
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(relative)
        })
        .display()
        .to_string()
}

fn render_project_readme(slug: &str, id: &str, targets: &[Target]) -> String {
    format!(
        "# {slug}\n\nFerro application scaffold.\n\n- Application id: `{id}`\n- Declared targets: {}\n\nRun `ferro doctor --project .` to inspect this project. On macOS, run `ferro run --project .` to launch the native development runtime. Packaging is not yet implemented.\n",
        join_targets(targets)
    )
}

fn join_targets(targets: &[Target]) -> String {
    targets
        .iter()
        .map(|target| target.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use ferro_project::ProjectManifest;

    use super::{Cli, Command, init_project, project_slug, run_project_with};

    #[test]
    fn initializes_a_desktop_project() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let project = directory.path().join("Atlas App");

        init_project(&project, None, ferro_sdk::Target::all().to_vec()).expect("create project");

        let manifest = ProjectManifest::load(project.join("ferro.toml")).expect("project manifest");
        assert_eq!(manifest.application().id(), "dev.ferro.atlas-app");
        assert_eq!(manifest.targets(), ferro_sdk::Target::all());
        assert!(project.join("crates/atlas-app-core/src/lib.rs").is_file());
        assert!(project.join("apps/atlas-app-macos/src/main.rs").is_file());
        assert!(project.join("apps/atlas-app-windows/src/main.rs").is_file());
        assert!(project.join("apps/atlas-app-linux/src/main.rs").is_file());
        assert!(
            std::fs::read_to_string(project.join("apps/atlas-app-macos/Cargo.toml"))
                .expect("macOS manifest")
                .contains("ferro-macos-runtime")
        );
        assert!(
            std::fs::read_to_string(project.join("apps/atlas-app-macos/src/main.rs"))
                .expect("macOS source")
                .contains("ferro_macos_runtime::run")
        );
    }

    #[test]
    fn refuses_an_existing_project_directory() {
        let directory = tempfile::tempdir().expect("temporary directory");
        assert!(init_project(directory.path(), None, ferro_sdk::Target::all().to_vec()).is_err());
    }

    #[test]
    fn parses_default_desktop_targets() {
        let cli = Cli::try_parse_from(["ferro", "init", "atlas"]).expect("parse command");
        let Command::Init { targets, .. } = cli.command else {
            panic!("expected init command");
        };
        assert_eq!(targets, ferro_sdk::Target::all());
    }

    #[test]
    fn derives_a_safe_project_slug() {
        assert_eq!(
            project_slug(std::path::Path::new("Atlas App!")),
            Ok("atlas-app".to_owned())
        );
    }

    #[test]
    fn prepares_a_macos_run_for_a_declared_target() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let project = directory.path().join("atlas");
        init_project(&project, None, vec![ferro_sdk::Target::Macos]).expect("create project");

        #[cfg(target_os = "macos")]
        run_project_with(&project, ferro_sdk::Target::Macos, |command| {
            assert_eq!(command.get_program(), "cargo");
            assert_eq!(
                command.get_args().collect::<Vec<_>>(),
                ["run", "--package", "atlas-macos"]
            );
            assert_eq!(command.get_current_dir(), Some(project.as_path()));
            Ok(())
        })
        .expect("prepare macOS command");
    }

    #[test]
    fn rejects_a_target_not_declared_by_the_project() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let project = directory.path().join("atlas");
        init_project(&project, None, vec![ferro_sdk::Target::Macos]).expect("create project");

        let error = run_project_with(&project, ferro_sdk::Target::Windows, |_| Ok(()))
            .expect_err("Windows was not declared");
        assert!(error.contains("does not declare the windows target"));
    }

    #[test]
    fn rejects_a_declared_target_without_a_development_runtime() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let project = directory.path().join("atlas");
        init_project(&project, None, vec![ferro_sdk::Target::Windows]).expect("create project");

        let error = run_project_with(&project, ferro_sdk::Target::Windows, |_| Ok(()))
            .expect_err("Windows does not have a runtime");
        assert!(error.contains("windows target is declared but has no Ferro development runtime"));
    }

    #[test]
    fn reports_an_invalid_project_manifest() {
        let directory = tempfile::tempdir().expect("temporary directory");
        std::fs::write(directory.path().join("ferro.toml"), "not = [valid")
            .expect("write invalid manifest");

        let error = run_project_with(directory.path(), ferro_sdk::Target::Macos, |_| Ok(()))
            .expect_err("invalid manifest must be rejected");
        assert!(error.contains("invalid Ferro manifest"));
    }

    #[test]
    fn reports_a_cargo_failure() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let project = directory.path().join("atlas");
        init_project(&project, None, vec![ferro_sdk::Target::Macos]).expect("create project");

        #[cfg(target_os = "macos")]
        {
            let error = run_project_with(&project, ferro_sdk::Target::Macos, |_| {
                Err("Cargo failed while running the macos target (exit status: 1)".to_owned())
            })
            .expect_err("Cargo failure must reach the caller");
            assert!(error.contains("Cargo failed"));
        }
    }
}
