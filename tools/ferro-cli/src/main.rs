//! The Ferro developer command-line interface.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::thread;
use std::time::{Duration, SystemTime};

use clap::{Parser, Subcommand, ValueEnum};
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
        #[arg(long, value_delimiter = ',', default_value = "macos")]
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
    /// Build a native application bundle.
    Build {
        /// Target to build.
        target: Target,
        /// Project directory containing ferro.toml.
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Build an optimized release bundle instead of a debug bundle.
        #[arg(long)]
        release: bool,
        /// Directory in which to write the .app bundle.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Override the macOS signing identity declared in ferro.toml.
        #[arg(long)]
        sign: Option<String>,
    },
    /// Create a signed macOS distribution artifact.
    Package {
        /// Target to package.
        target: Target,
        /// Project directory containing ferro.toml.
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Distribution architecture. Only Apple Silicon is supported.
        #[arg(long, value_enum)]
        arch: MacosArchitecture,
        /// Distribution container format.
        #[arg(long, value_enum, default_value_t = PackageFormat::Dmg)]
        format: PackageFormat,
        /// Directory in which to write the signed app and distribution artifact.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Developer ID Application signing identity.
        #[arg(long)]
        sign: Option<String>,
        /// Sign with a local ad-hoc identity (`-`) instead of a Developer ID certificate.
        #[arg(long, conflicts_with = "notarize")]
        ad_hoc: bool,
        /// Submit the artifact to Apple's notary service and staple its ticket.
        #[arg(long)]
        notarize: bool,
        /// Keychain profile previously stored with xcrun notarytool.
        #[arg(long, requires = "notarize")]
        notary_profile: Option<String>,
    },
    /// Build, launch, and restart a native target when its sources change.
    Dev {
        /// Project directory containing ferro.toml.
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Declared target to develop.
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
        Command::Build {
            target,
            project,
            release,
            output,
            sign,
        } => build_project(
            &project,
            target,
            BuildOptions {
                release,
                output,
                signing_identity: sign,
                target_triple: None,
            },
        )
        .map(|bundle| println!("Built {}", bundle.path.display())),
        Command::Dev { project, target } => develop_project(&project, target),
        Command::Package {
            target,
            project,
            arch,
            format,
            output,
            sign,
            ad_hoc,
            notarize,
            notary_profile,
        } => package_project(
            &project,
            target,
            PackageOptions {
                arch,
                format,
                output,
                signing_identity: sign,
                ad_hoc,
                notarize,
                notary_profile,
            },
        )
        .map(|artifact| println!("Packaged {}", artifact.display())),
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
    let mut manifest =
        ProjectManifest::new(&slug, id, targets).map_err(|error| error.to_string())?;
    if manifest.targets().contains(&Target::Macos) {
        manifest = manifest
            .with_macos_assets(
                Some(PathBuf::from("assets/macos/AppIcon.icns")),
                Some(PathBuf::from("assets/macos/resources")),
            )
            .map_err(|error| error.to_string())?;
    }

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
    if manifest.targets().contains(&Target::Macos) {
        write_file(&path.join("assets/macos/resources/.gitkeep"), "")?;
        write_file(
            &path.join("assets/macos/README.md"),
            "# macOS distribution assets\n\nAdd a production `AppIcon.icns` before running `ferro package macos --arch arm64`. Files in `resources/` are copied to `Contents/Resources`. Keep them static data: frameworks, plug-ins, executables and symlinks are not supported in this distribution phase.\n",
        )?;
    }
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
    let mut failures = Vec::new();
    inspect_tool("Rust", "rustc", &["--version"], &mut failures);
    inspect_tool("Cargo", "cargo", &["--version"], &mut failures);
    if std::env::consts::OS == "macos" {
        inspect_tool("Xcode", "xcodebuild", &["-version"], &mut failures);
        inspect_tool(
            "Code signing",
            "xcrun",
            &["--find", "codesign"],
            &mut failures,
        );
        inspect_tool(
            "Notary tool",
            "xcrun",
            &["--find", "notarytool"],
            &mut failures,
        );
        inspect_tool("Stapler", "xcrun", &["--find", "stapler"], &mut failures);
        inspect_tool(
            "Disk image tool",
            "xcrun",
            &["--find", "hdiutil"],
            &mut failures,
        );
    } else {
        failures.push("macOS builds require a macOS host with Xcode".to_owned());
        println!("- Xcode: unavailable (macOS host required)");
        println!("- Code signing: unavailable (macOS host required)");
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
            if manifest.targets().contains(&Target::Macos) {
                let slug = project_slug(project)?;
                if let Some(host) = resolve_macos_host_dir(project, &slug) {
                    println!("- macOS host: ready ({})", host.display());
                } else {
                    let expected = project.join("apps").join(format!("{slug}-macos"));
                    let message = format!(
                        "macOS host is missing (checked {}, {}, and {})",
                        expected.display(),
                        project.join("macos").display(),
                        project.display()
                    );
                    println!("- macOS host: invalid ({message})");
                    failures.push(message);
                }
            }
            inspect_signing_identity(manifest.macos().signing_identity(), &mut failures);
            inspect_distribution_configuration(project, &manifest);
        }
        Err(ProjectError::Read { error, .. }) if error.kind() == std::io::ErrorKind::NotFound => {
            println!("- Project: no ferro.toml at {}", manifest_path.display());
        }
        Err(error) => failures.push(error.to_string()),
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "doctor found {} issue(s): {}",
            failures.len(),
            failures.join("; ")
        ))
    }
}

fn run_project(project: &Path, target: Target) -> Result<(), String> {
    let bundle = build_project(project, target, BuildOptions::default())?;
    let status = ProcessCommand::new(&bundle.executable)
        .current_dir(project)
        .status()
        .map_err(|error| format!("could not launch {}: {error}", bundle.path.display()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "the {target} application exited unsuccessfully ({status})"
        ))
    }
}

/// macOS architecture accepted for distributable artifacts.
#[derive(Clone, Copy, Debug, ValueEnum)]
enum MacosArchitecture {
    /// Apple Silicon, compiled as aarch64-apple-darwin.
    Arm64,
}

impl MacosArchitecture {
    const fn target_triple(self) -> &'static str {
        match self {
            Self::Arm64 => "aarch64-apple-darwin",
        }
    }

    const fn display_name(self) -> &'static str {
        match self {
            Self::Arm64 => "arm64",
        }
    }
}

/// Container format for a direct macOS distribution.
#[derive(Clone, Copy, Debug, ValueEnum)]
enum PackageFormat {
    /// Apple disk image.
    Dmg,
    /// ZIP archive containing the app bundle.
    Zip,
}

impl PackageFormat {
    const fn extension(self) -> &'static str {
        match self {
            Self::Dmg => "dmg",
            Self::Zip => "zip",
        }
    }
}

#[derive(Clone, Debug, Default)]
struct BuildOptions {
    release: bool,
    output: Option<PathBuf>,
    signing_identity: Option<String>,
    target_triple: Option<&'static str>,
}

#[derive(Clone, Debug)]
struct PackageOptions {
    arch: MacosArchitecture,
    format: PackageFormat,
    output: Option<PathBuf>,
    signing_identity: Option<String>,
    ad_hoc: bool,
    notarize: bool,
    notary_profile: Option<String>,
}

#[derive(Clone, Debug)]
struct BuiltBundle {
    path: PathBuf,
    executable: PathBuf,
}

fn build_project(
    project: &Path,
    target: Target,
    options: BuildOptions,
) -> Result<BuiltBundle, String> {
    let (manifest, slug) = load_macos_project(project, target)?;
    let package = manifest
        .macos()
        .package()
        .map_or_else(|| format!("{slug}-{target}"), str::to_owned);
    let mut command = ProcessCommand::new("cargo");
    command.args(["build", "--package", &package]);
    if let Some(target_triple) = options.target_triple {
        command.args(["--target", target_triple]);
        command.env("RUSTFLAGS", "-C target-cpu=apple-m1");
    }
    if options.release {
        command.arg("--release");
    }
    let status = command
        .current_dir(project)
        .status()
        .map_err(|error| format!("could not start Cargo: {error}"))?;
    if !status.success() {
        return Err(format!(
            "Cargo failed while building the {target} target ({status})"
        ));
    }

    let profile = if options.release { "release" } else { "debug" };
    let target_root = cargo_target_directory(project);
    let executable_directory = options.target_triple.map_or_else(
        || target_root.join(profile),
        |target_triple| target_root.join(target_triple).join(profile),
    );
    let executable = executable_directory.join(&package);
    if !executable.is_file() {
        return Err(format!(
            "Cargo completed but did not produce the expected executable `{}`",
            executable.display()
        ));
    }
    let output = options
        .output
        .unwrap_or_else(|| target_root.join("ferro").join("macos").join(profile));
    assemble_macos_bundle(
        &output,
        manifest.application().name(),
        manifest.application().id(),
        &package,
        &executable,
        options
            .signing_identity
            .as_deref()
            .or(manifest.macos().signing_identity()),
    )
}

fn package_project(
    project: &Path,
    target: Target,
    options: PackageOptions,
) -> Result<PathBuf, String> {
    let (manifest, _) = load_macos_project(project, target)?;
    if manifest.schema() != ferro_project::CURRENT_SCHEMA {
        return Err(
            "ferro package requires a schema 2 ferro.toml with app.version and app.build_number"
                .to_owned(),
        );
    }
    let version = manifest.version().expect("schema 2 requires a version");
    let build_number = manifest
        .build_number()
        .expect("schema 2 requires a build number");
    let icon = required_project_asset(project, manifest.macos().icon(), "macos.icon")?;
    validate_icns(&icon)?;
    let resources = required_project_asset(
        project,
        manifest.macos().resources_dir(),
        "macos.resources_dir",
    )?;
    if !resources.is_dir() {
        return Err(format!(
            "macos.resources_dir `{}` is not a directory",
            resources.display()
        ));
    }
    let signing_identity = if options.ad_hoc {
        "-"
    } else {
        let identity = options
            .signing_identity
            .as_deref()
            .or(manifest.macos().signing_identity())
            .ok_or_else(|| "ferro package requires a Developer ID Application identity via --sign or macos.signing_identity (or pass --ad-hoc)".to_owned())?;
        if !identity.starts_with("Developer ID Application:") {
            return Err("ferro package requires a Developer ID Application identity".to_owned());
        }
        identity
    };
    if options.notarize && options.notary_profile.is_none() {
        return Err("--notarize requires --notary-profile <keychain-profile>".to_owned());
    }

    let output = options
        .output
        .unwrap_or_else(|| project.join("dist").join("macos"));
    let bundle_output = output.join("bundle");
    let bundle = build_project(
        project,
        target,
        BuildOptions {
            release: true,
            output: Some(bundle_output),
            signing_identity: None,
            target_triple: Some(options.arch.target_triple()),
        },
    )?;
    prepare_distribution_bundle(&bundle, &icon, &resources, version, build_number)?;
    if options.ad_hoc {
        sign_bundle(&bundle.path, "-")?;
    } else {
        sign_distribution_bundle(&bundle.path, signing_identity)?;
    }
    verify_apple_silicon(&bundle.executable)?;

    fs::create_dir_all(&output).map_err(|error| {
        format!(
            "could not create distribution output `{}`: {error}",
            output.display()
        )
    })?;
    let artifact_stem = format!(
        "{}-{}-{}",
        safe_bundle_name(manifest.application().name()),
        version,
        options.arch.display_name()
    );
    let artifact = output.join(format!("{artifact_stem}.{}", options.format.extension()));
    create_distribution_artifact(
        options.format,
        &bundle.path,
        &artifact,
        manifest.application().name(),
    )?;
    if options.notarize {
        notarize_artifact(
            &artifact,
            options.notary_profile.as_deref().expect("checked above"),
        )?;
        match options.format {
            PackageFormat::Dmg => staple_and_validate(&artifact)?,
            PackageFormat::Zip => {
                staple_and_validate(&bundle.path)?;
                create_distribution_artifact(
                    PackageFormat::Zip,
                    &bundle.path,
                    &artifact,
                    manifest.application().name(),
                )?;
            }
        }
        assess_distribution(&artifact)?;
    }
    Ok(artifact)
}

fn required_project_asset(
    project: &Path,
    path: Option<&Path>,
    key: &str,
) -> Result<PathBuf, String> {
    let path = path.ok_or_else(|| format!("ferro package requires {key} in ferro.toml"))?;
    let resolved = project.join(path);
    if resolved.exists() {
        Ok(resolved)
    } else {
        Err(format!("{key} does not exist at `{}`", resolved.display()))
    }
}

fn validate_icns(icon: &Path) -> Result<(), String> {
    if icon.extension().is_none_or(|extension| extension != "icns") {
        return Err(format!(
            "macos.icon `{}` must be an .icns file",
            icon.display()
        ));
    }
    let bytes = fs::read(icon)
        .map_err(|error| format!("could not read icon `{}`: {error}", icon.display()))?;
    if bytes.len() < 8 || &bytes[..4] != b"icns" {
        return Err(format!(
            "macos.icon `{}` is not a valid ICNS file",
            icon.display()
        ));
    }
    Ok(())
}

fn prepare_distribution_bundle(
    bundle: &BuiltBundle,
    icon: &Path,
    resources: &Path,
    version: &str,
    build_number: u64,
) -> Result<(), String> {
    let contents = bundle.path.join("Contents");
    let resource_destination = contents.join("Resources");
    fs::create_dir_all(&resource_destination)
        .map_err(|error| format!("could not create bundle resources: {error}"))?;
    fs::copy(icon, resource_destination.join("AppIcon.icns"))
        .map_err(|error| format!("could not copy macOS icon: {error}"))?;
    copy_static_resources(resources, &resource_destination)?;
    let plist = distribution_info_plist(&bundle.path, version, build_number)?;
    fs::write(contents.join("Info.plist"), plist)
        .map_err(|error| format!("could not write distribution Info.plist: {error}"))
}

fn distribution_info_plist(
    bundle: &Path,
    version: &str,
    build_number: u64,
) -> Result<String, String> {
    let plist = fs::read_to_string(bundle.join("Contents/Info.plist"))
        .map_err(|error| format!("could not read bundle Info.plist: {error}"))?;
    let insertion = "  <key>CFBundleIconFile</key>\n  <string>AppIcon</string>\n  <key>LSMinimumSystemVersion</key>\n  <string>11.0</string>\n  <key>CFBundleSupportedPlatforms</key>\n  <array><string>MacOSX</string></array>\n".to_owned();
    let plist = plist.replace(
        "  <key>CFBundleShortVersionString</key>\n  <string>0.1.0</string>\n",
        &format!(
            "  <key>CFBundleShortVersionString</key>\n  <string>{}</string>\n",
            xml_escape(version)
        ),
    );
    let plist = plist.replace(
        "  <key>CFBundleVersion</key>\n  <string>1</string>\n",
        &format!("  <key>CFBundleVersion</key>\n  <string>{build_number}</string>\n"),
    );
    Ok(plist.replace("</dict>", &format!("{insertion}</dict>")))
}

fn copy_static_resources(source: &Path, destination: &Path) -> Result<(), String> {
    for entry in fs::read_dir(source)
        .map_err(|error| format!("could not read resources `{}`: {error}", source.display()))?
    {
        let entry = entry.map_err(|error| format!("could not inspect a resource: {error}"))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("could not inspect `{}`: {error}", path.display()))?;
        if file_type.is_symlink() {
            return Err(format!(
                "resources cannot contain symlinks (`{}`)",
                path.display()
            ));
        }
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            fs::create_dir_all(&target)
                .map_err(|error| format!("could not create resource directory: {error}"))?;
            copy_static_resources(&path, &target)?;
        } else if file_type.is_file() {
            if is_executable_file(&path)? {
                return Err(format!(
                    "resources cannot contain executable files (`{}`)",
                    path.display()
                ));
            }
            fs::copy(&path, &target).map_err(|error| {
                format!("could not copy resource `{}`: {error}", path.display())
            })?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> Result<bool, String> {
    use std::os::unix::fs::PermissionsExt;

    let mode = fs::metadata(path)
        .map_err(|error| format!("could not inspect resource `{}`: {error}", path.display()))?
        .permissions()
        .mode();
    Ok(mode & 0o111 != 0)
}

#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)]
fn is_executable_file(_path: &Path) -> Result<bool, String> {
    Ok(false)
}

fn sign_distribution_bundle(bundle: &Path, identity: &str) -> Result<(), String> {
    run_checked(
        ProcessCommand::new("codesign")
            .args([
                "--force",
                "--sign",
                identity,
                "--options",
                "runtime",
                "--timestamp",
            ])
            .arg(bundle),
        "could not sign Developer ID application",
    )?;
    run_checked(
        ProcessCommand::new("codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(bundle),
        "Developer ID signature verification failed",
    )
}

fn verify_apple_silicon(executable: &Path) -> Result<(), String> {
    let output = ProcessCommand::new("lipo")
        .args(["-archs"])
        .arg(executable)
        .output()
        .map_err(|error| format!("could not inspect bundle architecture: {error}"))?;
    if output.status.success()
        && String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .any(|arch| arch == "arm64")
    {
        Ok(())
    } else {
        Err(format!(
            "distribution bundle is not Apple Silicon arm64: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn create_distribution_artifact(
    format: PackageFormat,
    bundle: &Path,
    artifact: &Path,
    name: &str,
) -> Result<(), String> {
    if artifact.exists() {
        fs::remove_file(artifact)
            .map_err(|error| format!("could not replace `{}`: {error}", artifact.display()))?;
    }
    match format {
        PackageFormat::Dmg => run_checked(
            ProcessCommand::new("hdiutil")
                .args([
                    "create",
                    "-ov",
                    "-format",
                    "UDZO",
                    "-volname",
                    name,
                    "-srcfolder",
                ])
                .arg(bundle)
                .arg(artifact),
            "could not create distribution DMG",
        ),
        PackageFormat::Zip => run_checked(
            ProcessCommand::new("ditto")
                .args(["-c", "-k", "--keepParent"])
                .arg(bundle)
                .arg(artifact),
            "could not create distribution ZIP",
        ),
    }
}

fn notarize_artifact(artifact: &Path, profile: &str) -> Result<(), String> {
    let output = ProcessCommand::new("xcrun")
        .args(["notarytool", "submit"])
        .arg(artifact)
        .args([
            "--wait",
            "--keychain-profile",
            profile,
            "--output-format",
            "json",
        ])
        .output()
        .map_err(|error| format!("could not start notarytool: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let details = String::from_utf8_lossy(&output.stdout);
    let submission = json_string_field(&details, "id");
    if let Some(submission) = submission {
        let _ = ProcessCommand::new("xcrun")
            .args([
                "notarytool",
                "log",
                &submission,
                "--keychain-profile",
                profile,
            ])
            .status();
    }
    Err(format!(
        "notarytool rejected `{}`: {}",
        artifact.display(),
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

fn json_string_field(source: &str, key: &str) -> Option<String> {
    let marker = format!("\"{key}\":\"");
    source
        .split(&marker)
        .nth(1)?
        .split('"')
        .next()
        .map(str::to_owned)
}

fn staple_and_validate(path: &Path) -> Result<(), String> {
    run_checked(
        ProcessCommand::new("xcrun")
            .args(["stapler", "staple"])
            .arg(path),
        "could not staple notarization ticket",
    )?;
    run_checked(
        ProcessCommand::new("xcrun")
            .args(["stapler", "validate"])
            .arg(path),
        "could not validate notarization ticket",
    )
}

fn assess_distribution(artifact: &Path) -> Result<(), String> {
    run_checked(
        ProcessCommand::new("spctl")
            .args(["--assess", "--type", "open", "--verbose=4"])
            .arg(artifact),
        "Gatekeeper assessment failed",
    )
}

fn run_checked(command: &mut ProcessCommand, context: &str) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|error| format!("{context}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{context} ({status})"))
    }
}

fn load_macos_project(project: &Path, target: Target) -> Result<(ProjectManifest, String), String> {
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
    if resolve_macos_host_dir(project, &slug).is_none() {
        let expected = project.join("apps").join(format!("{slug}-macos"));
        return Err(format!(
            "generated macOS host is missing at {}",
            expected.display()
        ));
    }
    Ok((manifest, slug))
}

fn resolve_macos_host_dir(project: &Path, slug: &str) -> Option<PathBuf> {
    let candidates = [
        project.join("apps").join(format!("{slug}-macos")),
        project.join("macos"),
        project.to_path_buf(),
    ];
    candidates
        .into_iter()
        .find(|dir| dir.join("Cargo.toml").is_file() && dir.join("src/main.rs").is_file())
}

fn cargo_target_directory(project: &Path) -> PathBuf {
    if let Ok(output) = ProcessCommand::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(project)
        .output()
        && output.status.success()
        && let Ok(meta) = serde_json::from_slice::<serde_json::Value>(&output.stdout)
        && let Some(dir) = meta
            .get("target_directory")
            .and_then(serde_json::Value::as_str)
    {
        return PathBuf::from(dir);
    }
    project.join("target")
}

fn develop_project(project: &Path, target: Target) -> Result<(), String> {
    let mut bundle = build_project(project, target, BuildOptions::default())?;
    println!("Launching {}", bundle.path.display());
    let mut child = launch_bundle(&bundle, project)?;
    let mut fingerprint = watch_fingerprint(project)?;

    loop {
        thread::sleep(Duration::from_millis(300));
        let next = watch_fingerprint(project)?;
        if next != fingerprint {
            fingerprint = next;
            println!("Change detected; rebuilding macOS application…");
            match build_project(project, target, BuildOptions::default()) {
                Ok(next_bundle) => {
                    if child
                        .try_wait()
                        .map_err(|error| error.to_string())?
                        .is_none()
                    {
                        child
                            .kill()
                            .map_err(|error| format!("could not stop application: {error}"))?;
                        child
                            .wait()
                            .map_err(|error| format!("could not wait for application: {error}"))?;
                    }
                    bundle = next_bundle;
                    println!("Restarting {}", bundle.path.display());
                    child = launch_bundle(&bundle, project)?;
                }
                Err(error) => {
                    eprintln!(
                        "ferro dev: rebuild failed; keeping the running application: {error}"
                    );
                }
            }
        }
    }
}

fn launch_bundle(bundle: &BuiltBundle, project: &Path) -> Result<std::process::Child, String> {
    ProcessCommand::new(&bundle.executable)
        .current_dir(project)
        .spawn()
        .map_err(|error| format!("could not launch {}: {error}", bundle.path.display()))
}

fn assemble_macos_bundle(
    output: &Path,
    application_name: &str,
    application_id: &str,
    executable_name: &str,
    executable: &Path,
    signing_identity: Option<&str>,
) -> Result<BuiltBundle, String> {
    fs::create_dir_all(output).map_err(|error| {
        format!(
            "could not create bundle output `{}`: {error}",
            output.display()
        )
    })?;
    let bundle_name = safe_bundle_name(application_name);
    let bundle = output.join(format!("{bundle_name}.app"));
    let staging = output.join(format!(
        ".{bundle_name}.app.ferro-staging-{}",
        std::process::id()
    ));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|error| {
            format!(
                "could not clear staging bundle `{}`: {error}",
                staging.display()
            )
        })?;
    }
    let contents = staging.join("Contents");
    let macos = contents.join("MacOS");
    fs::create_dir_all(&macos)
        .map_err(|error| format!("could not create bundle contents: {error}"))?;
    fs::write(
        contents.join("Info.plist"),
        macos_info_plist(application_name, application_id, executable_name),
    )
    .map_err(|error| format!("could not write bundle Info.plist: {error}"))?;
    let bundled_executable = macos.join(executable_name);
    fs::copy(executable, &bundled_executable).map_err(|error| {
        format!(
            "could not copy executable `{}` into bundle: {error}",
            executable.display()
        )
    })?;

    if let Some(identity) = signing_identity {
        sign_bundle(&staging, identity)?;
    }
    if bundle.exists() {
        fs::remove_dir_all(&bundle).map_err(|error| {
            format!(
                "could not replace existing bundle `{}`: {error}",
                bundle.display()
            )
        })?;
    }
    fs::rename(&staging, &bundle)
        .map_err(|error| format!("could not finalize bundle `{}`: {error}", bundle.display()))?;
    Ok(BuiltBundle {
        executable: bundle.join("Contents").join("MacOS").join(executable_name),
        path: bundle,
    })
}

fn sign_bundle(bundle: &Path, identity: &str) -> Result<(), String> {
    let status = ProcessCommand::new("codesign")
        .args(["--force", "--sign", identity, "--timestamp=none"])
        .arg(bundle)
        .status()
        .map_err(|error| format!("could not start codesign: {error}"))?;
    if !status.success() {
        return Err(format!(
            "codesign failed for `{}` ({status})",
            bundle.display()
        ));
    }
    let status = ProcessCommand::new("codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(bundle)
        .status()
        .map_err(|error| format!("could not verify bundle signature: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "codesign verification failed for `{}` ({status})",
            bundle.display()
        ))
    }
}

fn macos_info_plist(application_name: &str, application_id: &str, executable_name: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>CFBundleDisplayName</key>\n  <string>{}</string>\n  <key>CFBundleExecutable</key>\n  <string>{}</string>\n  <key>CFBundleIdentifier</key>\n  <string>{}</string>\n  <key>CFBundleInfoDictionaryVersion</key>\n  <string>6.0</string>\n  <key>CFBundleName</key>\n  <string>{}</string>\n  <key>CFBundlePackageType</key>\n  <string>APPL</string>\n  <key>CFBundleShortVersionString</key>\n  <string>0.1.0</string>\n  <key>CFBundleVersion</key>\n  <string>1</string>\n  <key>NSHighResolutionCapable</key>\n  <true/>\n  <key>NSPrincipalClass</key>\n  <string>NSApplication</string>\n</dict>\n</plist>\n",
        xml_escape(application_name),
        xml_escape(executable_name),
        xml_escape(application_id),
        xml_escape(application_name)
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn safe_bundle_name(name: &str) -> String {
    let name = name
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '\0' => '-',
            _ => character,
        })
        .collect::<String>();
    if name.trim().is_empty() {
        "Ferro".to_owned()
    } else {
        name
    }
}

fn watch_fingerprint(project: &Path) -> Result<Vec<(PathBuf, SystemTime, u64)>, String> {
    let mut entries = Vec::new();
    collect_watch_entries(project, project, &mut entries)?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(entries)
}

fn collect_watch_entries(
    root: &Path,
    directory: &Path,
    entries: &mut Vec<(PathBuf, SystemTime, u64)>,
) -> Result<(), String> {
    for entry in fs::read_dir(directory)
        .map_err(|error| format!("could not inspect `{}`: {error}", directory.display()))?
    {
        let entry = entry.map_err(|error| format!("could not inspect project file: {error}"))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("could not inspect `{}`: {error}", path.display()))?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            let name = entry.file_name();
            if name == "target"
                || name == ".git"
                || name == "node_modules"
                || name == ".nx"
                || name == "dist"
            {
                continue;
            }
            collect_watch_entries(root, &path, entries)?;
        } else if file_type.is_file() && watched_file(root, &path) {
            let metadata = entry
                .metadata()
                .map_err(|error| format!("could not inspect `{}`: {error}", path.display()))?;
            entries.push((
                path.strip_prefix(root).unwrap_or(&path).to_owned(),
                metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                metadata.len(),
            ));
        }
    }
    Ok(())
}

fn watched_file(root: &Path, path: &Path) -> bool {
    if path.extension().is_some_and(|extension| extension == "rs") {
        return true;
    }
    matches!(
        path.strip_prefix(root)
            .ok()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str()),
        Some("Cargo.toml" | "Cargo.lock" | "ferro.toml")
    )
}

fn inspect_tool(label: &str, tool: &str, arguments: &[&str], failures: &mut Vec<String>) {
    match ProcessCommand::new(tool).args(arguments).output() {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let version = stdout.lines().next().unwrap_or("available");
            println!("- {label}: {version}");
        }
        Ok(output) => {
            let message = format!("{tool} {} failed ({})", arguments.join(" "), output.status);
            println!("- {label}: unavailable ({message})");
            failures.push(message);
        }
        Err(error) => {
            let message = format!("{tool} is unavailable ({error})");
            println!("- {label}: unavailable ({error})");
            failures.push(message);
        }
    }
}

fn inspect_signing_identity(identity: Option<&str>, failures: &mut Vec<String>) {
    let Some(identity) = identity else {
        println!("- Signing identity: not configured (local bundles remain unsigned)");
        return;
    };
    if std::env::consts::OS != "macos" {
        failures.push(format!("signing identity `{identity}` requires macOS"));
        return;
    }
    match ProcessCommand::new("security")
        .args(["find-identity", "-v", "-p", "codesigning"])
        .output()
    {
        Ok(output)
            if output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains(identity) =>
        {
            println!("- Signing identity: available ({identity})");
        }
        Ok(_) => {
            let message = format!(
                "configured signing identity `{identity}` is not available in the keychain"
            );
            println!("- Signing identity: unavailable ({message})");
            failures.push(message);
        }
        Err(error) => {
            let message = format!("could not inspect signing identities: {error}");
            println!("- Signing identity: unavailable ({message})");
            failures.push(message);
        }
    }
}

fn inspect_distribution_configuration(project: &Path, manifest: &ProjectManifest) {
    if manifest.schema() != ferro_project::CURRENT_SCHEMA {
        println!(
            "- Distribution: unavailable (schema 2 metadata required; local development remains supported)"
        );
        return;
    }
    let icon = manifest
        .macos()
        .icon()
        .map(|path| project.join(path))
        .filter(|path| path.is_file());
    let resources = manifest
        .macos()
        .resources_dir()
        .map(|path| project.join(path))
        .filter(|path| path.is_dir());
    match (icon, resources) {
        (Some(icon), Some(resources)) => println!(
            "- Distribution: ready (version {} build {}, icon {}, resources {})",
            manifest.version().unwrap_or("unknown"),
            manifest.build_number().unwrap_or_default(),
            icon.display(),
            resources.display()
        ),
        _ => println!(
            "- Distribution: incomplete (add the configured ICNS icon and resources directory before ferro package)"
        ),
    }
}

fn list_targets() {
    println!("Ferro targets");
    for target in Target::all() {
        println!("- {target}: {}", target_status(target));
    }
}

fn target_status(target: Target) -> &'static str {
    match target {
        Target::Macos => {
            "supported for local development, .app bundles, and arm64 Developer ID distribution on macOS"
        }
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
        "//! Portable state and reducer for `{slug}`.\n\nuse std::path::PathBuf;\n\n/// Ferro application metadata and portable reducer.\npub struct Application;\n\n/// State shared by every native host.\n#[derive(Clone, Debug, Default, Eq, PartialEq)]\npub struct AppState {{\n    launches: usize,\n    selected_path: Option<PathBuf>,\n    status: String,\n}}\n\nimpl AppState {{\n    /// Returns the current native-runtime status.\n    #[must_use]\n    pub fn status(&self) -> &str {{\n        &self.status\n    }}\n\n    /// Returns the latest path chosen through a native capability.\n    #[must_use]\n    pub fn selected_path(&self) -> Option<&std::path::Path> {{\n        self.selected_path.as_deref()\n    }}\n}}\n\n/// Events accepted by the portable reducer.\n#[derive(Clone, Debug, Eq, PartialEq)]\npub enum AppEvent {{\n    /// The native runtime completed launch.\n    Launched,\n    /// The user requested the native file picker.\n    OpenFile,\n    /// The file picker completed; `None` means cancellation.\n    FileDialogFinished(Option<PathBuf>),\n}}\n\n/// Work delegated to an active target host.\n#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub enum AppEffect {{\n    /// Present the platform's file-selection capability.\n    OpenFileDialog,\n}}\n\nimpl ferro_sdk::Application for Application {{\n    type State = AppState;\n    type Event = AppEvent;\n    type Effect = AppEffect;\n\n    fn descriptor() -> ferro_sdk::ApplicationDescriptor {{\n        ferro_sdk::ApplicationDescriptor::new(\"{slug}\", \"{id}\")\n    }}\n\n    fn initial_state() -> Self::State {{\n        AppState {{\n            status: \"Ready\".to_owned(),\n            ..AppState::default()\n        }}\n    }}\n\n    fn reduce(state: &mut Self::State, event: Self::Event) -> Vec<Self::Effect> {{\n        match event {{\n            AppEvent::Launched => {{\n                state.launches += 1;\n                state.status = format!(\"Running (launch #{{}})\", state.launches);\n                Vec::new()\n            }}\n            AppEvent::OpenFile => vec![AppEffect::OpenFileDialog],\n            AppEvent::FileDialogFinished(Some(path)) => {{\n                state.status = format!(\"Selected {{}}\", path.display());\n                state.selected_path = Some(path);\n                Vec::new()\n            }}\n            AppEvent::FileDialogFinished(None) => Vec::new(),\n        }}\n    }}\n}}\n"
    )
}

fn render_host_manifest(slug: &str, target: Target) -> String {
    let runtime = if target == Target::Macos {
        format!(
            "ferro-macos-runtime = {{ path = \"{}\" }}\nferro-storage = {{ path = \"{}\", optional = true }}\nobjc2 = \"0.6.5\"\nobjc2-app-kit = \"0.3.2\"\nobjc2-foundation = \"0.3.2\"\n\n[features]\npersistence = [\"dep:ferro-storage\"]\n",
            framework_crate_path("crates/ferro-macos-runtime"),
            framework_crate_path("crates/ferro-storage")
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
    let crate_ident = slug.replace('-', "_");
    if target == Target::Macos {
        return format!(
            "use std::cell::OnceCell;\n\nuse ferro_sdk::Lifecycle;\nuse objc2::MainThreadMarker;\nuse objc2::rc::Retained;\nuse objc2_app_kit::{{NSFont, NSTextField, NSWindow}};\nuse objc2_foundation::{{NSPoint, NSRect, NSSize, NSString}};\n\nuse {crate_ident}_core::{{AppEffect, AppEvent, AppState, Application}};\nuse ferro_macos_runtime::{{CommandId, FileDialogResult, MacosCapabilities, MacosCommand, MacosEventSender, MacosHost, OpenPanelMode}};\n\n#[derive(Default)]\nstruct MacosApp {{\n    status: OnceCell<Retained<NSTextField>>,\n    selected_path: OnceCell<Retained<NSTextField>>,\n}}\n\nimpl MacosHost<Application> for MacosApp {{\n    fn build(&mut self, window: &NSWindow, main_thread: MainThreadMarker, _sender: MacosEventSender<AppEvent>) {{\n        let content = window.contentView().expect(\"runtime windows have a content view\");\n        let title = NSTextField::labelWithString(&NSString::from_str(\"{slug}\"), main_thread);\n        title.setFrame(NSRect::new(NSPoint::new(32.0, 390.0), NSSize::new(736.0, 36.0)));\n        title.setFont(Some(&NSFont::systemFontOfSize(28.0)));\n        let status = NSTextField::labelWithString(&NSString::from_str(\"Starting…\"), main_thread);\n        status.setFrame(NSRect::new(NSPoint::new(32.0, 340.0), NSSize::new(736.0, 24.0)));\n        let selected_path = NSTextField::labelWithString(&NSString::from_str(\"No file selected\"), main_thread);\n        selected_path.setFrame(NSRect::new(NSPoint::new(32.0, 308.0), NSSize::new(736.0, 24.0)));\n        content.addSubview(&title);\n        content.addSubview(&status);\n        content.addSubview(&selected_path);\n        self.status.set(status).expect(\"host builds once\");\n        self.selected_path.set(selected_path).expect(\"host builds once\");\n    }}\n\n    fn render(&mut self, state: &AppState) {{\n        if let Some(status) = self.status.get() {{\n            status.setStringValue(&NSString::from_str(state.status()));\n        }}\n        if let Some(label) = self.selected_path.get() {{\n            let text = state.selected_path().map_or_else(|| \"No file selected\".to_owned(), |path| path.display().to_string());\n            label.setStringValue(&NSString::from_str(&text));\n        }}\n    }}\n\n    fn commands(&self, _state: &AppState) -> Vec<MacosCommand<AppEvent>> {{\n        vec![MacosCommand::new(CommandId::new(\"file.open\"), \"Open…\", AppEvent::OpenFile).shortcut('o')]\n    }}\n\n    fn lifecycle_event(&mut self, lifecycle: Lifecycle) -> Option<AppEvent> {{\n        (lifecycle == Lifecycle::Launched).then_some(AppEvent::Launched)\n    }}\n\n    fn handle_effect(&mut self, effect: AppEffect, capabilities: MacosCapabilities<AppEvent>) {{\n        match effect {{\n            AppEffect::OpenFileDialog => capabilities.open(OpenPanelMode::Files {{ multiple: false }}, |result| {{\n                let path = match result {{\n                    FileDialogResult::Selected(paths) => paths.into_iter().next(),\n                    FileDialogResult::Cancelled => None,\n                }};\n                AppEvent::FileDialogFinished(path)\n            }}),\n        }}\n    }}\n}}\n\nfn main() {{\n    #[cfg(feature = \"persistence\")]\n    let _liliadb_service = std::any::TypeId::of::<ferro_storage::LiliaApplicationStorage>();\n    ferro_macos_runtime::run::<Application, _>(MacosApp::default());\n}}\n"
        );
    }
    format!(
        "use ferro_sdk::Application as _;\n\nfn main() {{\n    let descriptor = {crate_ident}_core::Application::descriptor();\n    println!(\"{slug}: {target} host scaffold (runtime not implemented)\");\n    println!(\"Application id: {{}}\", descriptor.id());\n}}\n"
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
        "# {slug}\n\nFerro application scaffold.\n\n- Application id: `{id}`\n- Declared targets: {}\n\nOn macOS, `ferro doctor --project .` verifies Rust, Xcode and this project. `ferro run --project .` creates a Debug `.app` bundle and launches it. `ferro dev --project .` watches Rust and Cargo sources, then rebuilds and relaunches after a successful change. `ferro build macos --project .` writes a Debug bundle under `target/ferro/macos/debug`; add `--release` for an optimized bundle or `--output <directory>` to choose another directory.\n\nFor direct distribution, add a production `assets/macos/AppIcon.icns`, keep static files under `assets/macos/resources`, configure a `Developer ID Application` identity in `[macos]`, then run `ferro package macos --arch arm64 --notarize --notary-profile <profile>`. This creates a signed DMG by default; use `--format zip` when required. App Store sandboxing and entitlements are not part of this project.\n\nEnable optional LiliaDB-backed preferences and recents with `cargo run -p {slug}-macos --features persistence`. SQLite is not supported.\n",
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

    use super::{
        Cli, Command, assemble_macos_bundle, init_project, load_macos_project, macos_info_plist,
        prepare_distribution_bundle, project_slug, resolve_macos_host_dir, safe_bundle_name,
        watch_fingerprint,
    };

    #[test]
    fn initializes_a_macos_project() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let project = directory.path().join("Atlas App");

        init_project(&project, None, vec![ferro_sdk::Target::Macos]).expect("create project");

        let manifest = ProjectManifest::load(project.join("ferro.toml")).expect("project manifest");
        assert_eq!(manifest.application().id(), "dev.ferro.atlas-app");
        assert_eq!(manifest.schema(), 2);
        assert_eq!(manifest.version(), Some("0.1.0"));
        assert_eq!(manifest.build_number(), Some(1));
        assert_eq!(manifest.targets(), [ferro_sdk::Target::Macos]);
        assert!(project.join("crates/atlas-app-core/src/lib.rs").is_file());
        assert!(project.join("apps/atlas-app-macos/src/main.rs").is_file());
        assert!(!project.join("apps/atlas-app-windows/src/main.rs").exists());
        assert!(!project.join("apps/atlas-app-linux/src/main.rs").exists());
        assert!(
            std::fs::read_to_string(project.join("apps/atlas-app-macos/Cargo.toml"))
                .expect("macOS manifest")
                .contains("ferro-macos-runtime")
        );
        assert!(
            std::fs::read_to_string(project.join("apps/atlas-app-macos/src/main.rs"))
                .expect("macOS source")
                .contains("ferro_macos_runtime::run::<Application, _>")
        );
        let core = std::fs::read_to_string(project.join("crates/atlas-app-core/src/lib.rs"))
            .expect("portable core source");
        assert!(core.contains("pub struct AppState"));
        assert!(core.contains("fn reduce"));
        let macos = std::fs::read_to_string(project.join("apps/atlas-app-macos/src/main.rs"))
            .expect("macOS source");
        assert!(macos.contains("use atlas_app_core"));
        assert!(macos.contains("OpenPanelMode::Files"));
        let macos_manifest =
            std::fs::read_to_string(project.join("apps/atlas-app-macos/Cargo.toml"))
                .expect("macOS manifest");
        assert!(macos_manifest.contains("persistence = [\"dep:ferro-storage\"]"));
        assert!(!macos_manifest.contains("sqlite"));
        assert!(
            std::fs::read_to_string(project.join("README.md"))
                .expect("project README")
                .contains("ferro build macos")
        );
        assert!(
            std::fs::read_to_string(project.join("README.md"))
                .expect("project README")
                .contains("ferro package macos --arch arm64")
        );
        assert!(project.join("assets/macos/resources/.gitkeep").is_file());
        assert!(project.join("assets/macos/README.md").is_file());
    }

    #[test]
    fn refuses_an_existing_project_directory() {
        let directory = tempfile::tempdir().expect("temporary directory");
        assert!(init_project(directory.path(), None, vec![ferro_sdk::Target::Macos]).is_err());
    }

    #[test]
    fn defaults_init_to_macos() {
        let cli = Cli::try_parse_from(["ferro", "init", "atlas"]).expect("parse command");
        let Command::Init { targets, .. } = cli.command else {
            panic!("expected init command");
        };
        assert_eq!(targets, [ferro_sdk::Target::Macos]);
    }

    #[test]
    fn parses_build_and_dev_commands() {
        let cli = Cli::try_parse_from([
            "ferro",
            "build",
            "macos",
            "--release",
            "--output",
            "dist",
            "--sign",
            "Developer ID Application: Atlas",
        ])
        .expect("parse build command");
        let Command::Build {
            target,
            release,
            output,
            sign,
            ..
        } = cli.command
        else {
            panic!("expected build command");
        };
        assert_eq!(target, ferro_sdk::Target::Macos);
        assert!(release);
        assert_eq!(output, Some(std::path::PathBuf::from("dist")));
        assert_eq!(sign.as_deref(), Some("Developer ID Application: Atlas"));

        let cli = Cli::try_parse_from(["ferro", "dev"]).expect("parse dev command");
        let Command::Dev { target, .. } = cli.command else {
            panic!("expected dev command");
        };
        assert_eq!(target, ferro_sdk::Target::Macos);
    }

    #[test]
    fn parses_apple_silicon_package_command() {
        let cli = Cli::try_parse_from([
            "ferro",
            "package",
            "macos",
            "--arch",
            "arm64",
            "--format",
            "zip",
            "--notarize",
            "--notary-profile",
            "ferro-notary",
        ])
        .expect("parse package command");
        let Command::Package {
            notarize,
            notary_profile,
            ..
        } = cli.command
        else {
            panic!("expected package command");
        };
        assert!(notarize);
        assert_eq!(notary_profile.as_deref(), Some("ferro-notary"));
    }

    #[test]
    fn derives_a_safe_project_slug() {
        assert_eq!(
            project_slug(std::path::Path::new("Atlas App!")),
            Ok("atlas-app".to_owned())
        );
    }

    #[test]
    fn assembles_a_bundle_with_expected_metadata() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let executable = directory.path().join("atlas-macos");
        std::fs::write(&executable, "executable").expect("fake executable");
        let bundle = assemble_macos_bundle(
            directory.path(),
            "Atlas App",
            "dev.ferro.atlas",
            "atlas-macos",
            &executable,
            None,
        )
        .expect("assemble bundle");

        assert_eq!(bundle.path, directory.path().join("Atlas App.app"));
        assert!(bundle.executable.is_file());
        let plist =
            std::fs::read_to_string(bundle.path.join("Contents/Info.plist")).expect("bundle plist");
        assert!(plist.contains("dev.ferro.atlas"));
        assert!(plist.contains("atlas-macos"));
    }

    #[test]
    fn renders_a_safe_info_plist() {
        let plist = macos_info_plist("Atlas & Co", "dev.ferro.atlas", "atlas-macos");
        assert!(plist.contains("Atlas &amp; Co"));
        assert!(plist.contains("CFBundlePackageType"));
    }

    #[test]
    fn adds_distribution_assets_and_release_metadata() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let executable = directory.path().join("atlas-macos");
        std::fs::write(&executable, "executable").expect("fake executable");
        let bundle = assemble_macos_bundle(
            directory.path(),
            "Atlas",
            "dev.ferro.atlas",
            "atlas-macos",
            &executable,
            None,
        )
        .expect("assemble bundle");
        let icon = directory.path().join("AppIcon.icns");
        std::fs::write(&icon, b"icns\0\0\0\x08").expect("icns header");
        let resources = directory.path().join("resources");
        std::fs::create_dir_all(&resources).expect("resources directory");
        std::fs::write(resources.join("welcome.txt"), "hello").expect("resource");

        prepare_distribution_bundle(&bundle, &icon, &resources, "1.2.3", 42)
            .expect("prepare distribution bundle");
        let contents = bundle.path.join("Contents");
        assert!(contents.join("Resources/AppIcon.icns").is_file());
        assert!(contents.join("Resources/welcome.txt").is_file());
        let plist = std::fs::read_to_string(contents.join("Info.plist")).expect("plist");
        assert!(plist.contains("<string>1.2.3</string>"));
        assert!(plist.contains("<string>42</string>"));
        assert!(plist.contains("CFBundleIconFile"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn creates_distribution_zip_and_dmg() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let executable = directory.path().join("atlas-macos");
        std::fs::write(&executable, "executable").expect("fake executable");
        let bundle = assemble_macos_bundle(
            directory.path(),
            "Atlas",
            "dev.ferro.atlas",
            "atlas-macos",
            &executable,
            None,
        )
        .expect("assemble bundle");
        let zip = directory.path().join("Atlas.zip");
        let dmg = directory.path().join("Atlas.dmg");

        super::create_distribution_artifact(super::PackageFormat::Zip, &bundle.path, &zip, "Atlas")
            .expect("create zip");
        super::create_distribution_artifact(super::PackageFormat::Dmg, &bundle.path, &dmg, "Atlas")
            .expect("create dmg");

        assert!(zip.is_file());
        assert!(dmg.is_file());
    }

    #[test]
    fn fingerprints_project_sources_without_target_artifacts() {
        let directory = tempfile::tempdir().expect("temporary directory");
        std::fs::write(directory.path().join("ferro.toml"), "schema = 1").expect("manifest");
        std::fs::create_dir_all(directory.path().join("src")).expect("source directory");
        std::fs::write(directory.path().join("src/lib.rs"), "pub fn atlas() {}").expect("source");
        std::fs::create_dir_all(directory.path().join("target")).expect("target directory");
        std::fs::write(directory.path().join("target/generated.rs"), "ignored")
            .expect("target artifact");

        let fingerprint = watch_fingerprint(directory.path()).expect("fingerprint");
        assert_eq!(fingerprint.len(), 2);
    }

    #[test]
    fn rejects_macos_build_for_an_undeclared_target_before_launching_cargo() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let project = directory.path().join("atlas");
        init_project(&project, None, vec![ferro_sdk::Target::Windows]).expect("create project");

        let error = load_macos_project(&project, ferro_sdk::Target::Macos)
            .expect_err("undeclared target must be rejected");
        assert!(error.contains("does not declare the macos target"));
    }

    #[test]
    fn rejects_a_declared_target_without_a_runtime_before_launching_cargo() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let project = directory.path().join("atlas");
        init_project(&project, None, vec![ferro_sdk::Target::Windows]).expect("create project");

        let error = load_macos_project(&project, ferro_sdk::Target::Windows)
            .expect_err("unsupported target must be rejected");
        assert!(error.contains("windows target is declared but has no Ferro development runtime"));
    }

    #[test]
    fn normalizes_unsafe_bundle_names() {
        assert_eq!(safe_bundle_name("Atlas/App"), "Atlas-App");
    }

    #[test]
    fn resolves_monorepo_subproject_host_directory() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let subproject = directory.path().join("apps").join("worklens");
        std::fs::create_dir_all(subproject.join("macos/src")).expect("create macos subproject");
        std::fs::write(
            subproject.join("ferro.toml"),
            "schema = 2\n\n[app]\nname = \"Worklens\"\nid = \"dev.worklens.desktop\"\ntargets = [\"macos\"]\nversion = \"0.1.0\"\nbuild_number = 1\n\n[macos]\npackage = \"worklens-macos\"\n",
        )
        .expect("manifest");
        std::fs::write(
            subproject.join("macos/Cargo.toml"),
            "[package]\nname = \"worklens-macos\"\n",
        )
        .expect("cargo");
        std::fs::write(subproject.join("macos/src/main.rs"), "fn main() {}\n").expect("main");

        let host = resolve_macos_host_dir(&subproject, "worklens")
            .expect("resolve monorepo subproject host");
        assert_eq!(host, subproject.join("macos"));
    }

    #[test]
    fn parses_ad_hoc_package_command() {
        let cli = Cli::try_parse_from(["ferro", "package", "macos", "--arch", "arm64", "--ad-hoc"])
            .expect("parse ad-hoc package command");
        let Command::Package {
            ad_hoc, notarize, ..
        } = cli.command
        else {
            panic!("expected package command");
        };
        assert!(ad_hoc);
        assert!(!notarize);
    }
}
