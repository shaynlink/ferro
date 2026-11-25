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
        &render_project_readme(&slug, manifest.application().id()),
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
    println!("Next: ferro doctor --project {}", path.display());
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

fn list_targets() {
    println!("Ferro targets");
    for target in Target::all() {
        println!("- {target}: {}", target_status(target));
    }
}

fn target_status(target: Target) -> &'static str {
    match target {
        Target::Macos => "reference host exists; packaging is not yet supported",
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
        "[package]\nname = \"{slug}-core\"\nversion.workspace = true\nedition.workspace = true\nrust-version.workspace = true\n\n[lints.rust]\nmissing_docs = \"warn\"\n"
    )
}

fn render_core_source(slug: &str, id: &str) -> String {
    format!(
        "//! Shared application logic for `{slug}`.\n\n/// Returns the stable application identifier.\n#[must_use]\npub const fn application_id() -> &'static str {{\n    \"{id}\"\n}}\n"
    )
}

fn render_host_manifest(slug: &str, target: Target) -> String {
    format!(
        "[package]\nname = \"{slug}-{target}\"\nversion.workspace = true\nedition.workspace = true\nrust-version.workspace = true\n\n[dependencies]\n{slug}-core = {{ path = \"../../crates/{slug}-core\" }}\n"
    )
}

fn render_host_source(slug: &str, target: Target) -> String {
    format!(
        "fn main() {{\n    println!(\"{slug}: {target} host scaffold (runtime not implemented)\");\n    println!(\"Application id: {{}}\", {slug}_core::application_id());\n}}\n"
    )
}

fn render_project_readme(slug: &str, id: &str) -> String {
    format!(
        "# {slug}\n\nFerro application scaffold.\n\n- Application id: `{id}`\n- Declared targets: macOS, Windows, Linux\n\nRun `ferro doctor --project .` to inspect this project. Target runtimes and packaging are not yet implemented.\n"
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

    use super::{Cli, Command, init_project, project_slug};

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
}
