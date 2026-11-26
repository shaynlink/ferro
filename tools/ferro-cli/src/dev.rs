//! Hot-reload development loop with source/asset classification, debouncing, incremental
//! resource hot-sync, state-snapshot preservation, and optional test-on-save.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command as ProcessCommand};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use clap::Args;
use ferro_sdk::Target;

use crate::http_client::{discover_devtools_targets, http_post_json};
use crate::{BuildOptions, BuiltBundle, build_project, load_macos_project};

#[derive(Args, Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "CLI flag options map directly to independent developer toggles"
)]
pub(crate) struct DevArgs {
    /// Project directory containing ferro.toml.
    #[arg(long, default_value = ".")]
    project: PathBuf,
    /// Declared target to develop.
    #[arg(long, default_value = "macos")]
    pub(crate) target: Target,
    /// Build with `--release` instead of debug profile.
    #[arg(long)]
    release: bool,
    /// Comma-separated Cargo features passed to the host build.
    #[arg(long, value_delimiter = ',')]
    features: Vec<String>,
    /// File-change debounce window in milliseconds.
    #[arg(long, default_value_t = 250)]
    debounce_ms: u64,
    /// Preserve application snapshot file across hot-restarts.
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    preserve_state: bool,
    /// Run `cargo test` before relaunching on Rust source changes.
    #[arg(long)]
    test_on_save: bool,
    /// Disable zero-recompile asset hot-sync into `.app/Contents/Resources`.
    #[arg(long)]
    no_asset_reload: bool,
    /// Explicit localhost port for the embedded `DevTools` server.
    #[arg(long)]
    devtools_port: Option<u16>,
    /// Build once, verify launch readiness, and exit immediately (useful in scripts/CI).
    #[arg(long)]
    once: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WatchedKind {
    Source,
    Asset,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WatchedEntry {
    pub(crate) path: PathBuf,
    pub(crate) modified: SystemTime,
    pub(crate) len: u64,
    pub(crate) kind: WatchedKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ChangeClassification {
    None,
    AssetsOnly(Vec<PathBuf>),
    Sources(Vec<PathBuf>),
}

pub(crate) fn run(args: &DevArgs) -> Result<(), String> {
    let (manifest, _) = load_macos_project(&args.project, args.target)?;
    let state_file = args
        .preserve_state
        .then(|| std::env::temp_dir().join(format!("ferro-dev-state-{}.json", std::process::id())));
    let resources_dir = manifest
        .macos()
        .resources_dir()
        .map(|rel| args.project.join(rel));

    let build_start = Instant::now();
    let mut bundle = build_dev_bundle(args)?;
    println!(
        "Built {} in {} ms",
        bundle.path.display(),
        build_start.elapsed().as_millis()
    );
    if !args.no_asset_reload
        && let Some(ref res_dir) = resources_dir
        && res_dir.is_dir()
    {
        sync_assets_into_bundle(res_dir, &bundle.path)?;
    }

    println!(
        "Launching {} (hot-reload + DevTools active)…",
        bundle.path.display()
    );
    let mut child = spawn_dev_app(&bundle, args, state_file.as_deref())?;

    if args.once {
        thread::sleep(Duration::from_millis(250));
        stop_child_gracefully(&mut child);
        if let Some(ref file) = state_file {
            let _ = fs::remove_file(file);
        }
        return Ok(());
    }

    let mut baseline = collect_dev_fingerprint(&args.project, resources_dir.as_deref())?;
    let debounce = Duration::from_millis(args.debounce_ms);

    loop {
        thread::sleep(Duration::from_millis(200));
        let next = collect_dev_fingerprint(&args.project, resources_dir.as_deref())?;
        let changes = classify_changes(&baseline, &next);
        if matches!(changes, ChangeClassification::None) {
            continue;
        }
        thread::sleep(debounce);
        let settled = collect_dev_fingerprint(&args.project, resources_dir.as_deref())?;
        let classification = classify_changes(&baseline, &settled);
        baseline = settled;

        match classification {
            ChangeClassification::None => {}
            ChangeClassification::AssetsOnly(paths) if !args.no_asset_reload => {
                handle_asset_reload(&paths, resources_dir.as_deref(), &bundle.path, child.id());
            }
            ChangeClassification::AssetsOnly(paths) | ChangeClassification::Sources(paths) => {
                handle_source_reload(
                    &paths,
                    args,
                    resources_dir.as_deref(),
                    state_file.as_deref(),
                    &mut bundle,
                    &mut child,
                )?;
            }
        }
    }
}

fn handle_asset_reload(
    paths: &[PathBuf],
    resources_dir: Option<&Path>,
    bundle_path: &Path,
    child_pid: u32,
) {
    println!(
        "Asset change detected ({} file(s)); hot-reloading without recompile…",
        paths.len()
    );
    if let Some(res_dir) = resources_dir
        && res_dir.is_dir()
        && let Err(error) = sync_assets_into_bundle(res_dir, bundle_path)
    {
        eprintln!("ferro dev: asset sync failed: {error}");
        return;
    }
    if notify_running_app_reload(child_pid) {
        println!("Hot-reloaded assets into running instance (pid {child_pid}).");
    } else {
        println!("Synced assets into bundle (DevTools endpoint not yet reachable).");
    }
}

fn handle_source_reload(
    paths: &[PathBuf],
    args: &DevArgs,
    resources_dir: Option<&Path>,
    state_file: Option<&Path>,
    bundle: &mut BuiltBundle,
    child: &mut Child,
) -> Result<(), String> {
    println!(
        "Source change detected ({} file(s)); rebuilding…",
        paths.len()
    );
    if args.test_on_save && !run_dev_tests(&args.project) {
        eprintln!("ferro dev: tests failed; keeping running application until fixed.");
        return Ok(());
    }
    let rebuild_start = Instant::now();
    match build_dev_bundle(args) {
        Ok(next_bundle) => {
            if !args.no_asset_reload
                && let Some(res_dir) = resources_dir
                && res_dir.is_dir()
            {
                let _ = sync_assets_into_bundle(res_dir, &next_bundle.path);
            }
            stop_child_gracefully(child);
            *bundle = next_bundle;
            *child = spawn_dev_app(bundle, args, state_file)?;
            println!(
                "Hot-restarted {} in {} ms (pid {}).",
                bundle.path.display(),
                rebuild_start.elapsed().as_millis(),
                child.id()
            );
        }
        Err(error) => {
            eprintln!("ferro dev: rebuild failed; keeping running application: {error}");
        }
    }
    Ok(())
}

fn build_dev_bundle(args: &DevArgs) -> Result<BuiltBundle, String> {
    build_project(
        &args.project,
        args.target,
        BuildOptions {
            release: args.release,
            features: args.features.clone(),
            ..BuildOptions::default()
        },
    )
}

fn spawn_dev_app(
    bundle: &BuiltBundle,
    args: &DevArgs,
    state_file: Option<&Path>,
) -> Result<Child, String> {
    let mut cmd = ProcessCommand::new(&bundle.executable);
    cmd.current_dir(&args.project).env("FERRO_DEVTOOLS", "1");
    if let Some(port) = args.devtools_port {
        cmd.env("FERRO_DEVTOOLS_PORT", port.to_string());
    }
    if let Some(path) = state_file {
        cmd.env("FERRO_DEV_STATE_FILE", path);
    }
    cmd.spawn()
        .map_err(|error| format!("could not launch {}: {error}", bundle.path.display()))
}

fn stop_child_gracefully(child: &mut Child) {
    if let Ok(Some(_)) = child.try_wait() {
        return;
    }
    #[cfg(unix)]
    {
        let _ = ProcessCommand::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status();
        for _ in 0..12 {
            thread::sleep(Duration::from_millis(50));
            if let Ok(Some(_)) = child.try_wait() {
                return;
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn notify_running_app_reload(pid: u32) -> bool {
    let targets = discover_devtools_targets();
    let Some(target) = targets.into_iter().find(|item| item.pid == pid) else {
        return false;
    };
    http_post_json(&target.url, "/dev/reload", "{}").is_ok()
}

fn run_dev_tests(project: &Path) -> bool {
    ProcessCommand::new("cargo")
        .args(["test", "--workspace"])
        .current_dir(project)
        .status()
        .is_ok_and(|status| status.success())
}

fn sync_assets_into_bundle(resources_dir: &Path, bundle_path: &Path) -> Result<(), String> {
    let dest = bundle_path.join("Contents/Resources");
    fs::create_dir_all(&dest)
        .map_err(|error| format!("could not create `{}`: {error}", dest.display()))?;
    copy_dir_contents(resources_dir, &dest)
}

fn copy_dir_contents(src: &Path, dst: &Path) -> Result<(), String> {
    for entry in
        fs::read_dir(src).map_err(|error| format!("could not read `{}`: {error}", src.display()))?
    {
        let entry = entry.map_err(|error| error.to_string())?;
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_symlink() {
            continue;
        }
        let target = dst.join(entry.file_name());
        if file_type.is_dir() {
            fs::create_dir_all(&target).map_err(|error| error.to_string())?;
            copy_dir_contents(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

pub(crate) fn collect_dev_fingerprint(
    project: &Path,
    resources_dir: Option<&Path>,
) -> Result<Vec<WatchedEntry>, String> {
    let mut entries = Vec::new();
    collect_entries_recursive(project, project, resources_dir, &mut entries)?;
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(entries)
}

fn collect_entries_recursive(
    root: &Path,
    directory: &Path,
    resources_dir: Option<&Path>,
    entries: &mut Vec<WatchedEntry>,
) -> Result<(), String> {
    for entry in fs::read_dir(directory)
        .map_err(|error| format!("could not inspect `{}`: {error}", directory.display()))?
    {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
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
            collect_entries_recursive(root, &path, resources_dir, entries)?;
        } else if file_type.is_file() {
            let kind = if resources_dir.is_some_and(|dir| path.starts_with(dir))
                || is_asset_path(root, &path)
            {
                Some(WatchedKind::Asset)
            } else if is_source_path(root, &path) {
                Some(WatchedKind::Source)
            } else {
                None
            };
            if let Some(kind) = kind {
                let metadata = entry.metadata().map_err(|error| error.to_string())?;
                entries.push(WatchedEntry {
                    path: path.strip_prefix(root).unwrap_or(&path).to_owned(),
                    modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    len: metadata.len(),
                    kind,
                });
            }
        }
    }
    Ok(())
}

fn is_source_path(root: &Path, path: &Path) -> bool {
    if path.extension().is_some_and(|ext| ext == "rs") {
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

fn is_asset_path(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    let is_under_assets = rel
        .components()
        .next()
        .is_some_and(|c| c.as_os_str() == "assets");
    is_under_assets
        && path.extension().is_some_and(|ext| {
            matches!(
                ext.to_str(),
                Some("json" | "toml" | "txt" | "md" | "css" | "html" | "svg" | "png")
            )
        })
}

pub(crate) fn classify_changes(
    prev: &[WatchedEntry],
    next: &[WatchedEntry],
) -> ChangeClassification {
    let prev_map: BTreeMap<&Path, (SystemTime, u64, WatchedKind)> = prev
        .iter()
        .map(|entry| {
            (
                entry.path.as_path(),
                (entry.modified, entry.len, entry.kind),
            )
        })
        .collect();
    let next_map: BTreeMap<&Path, (SystemTime, u64, WatchedKind)> = next
        .iter()
        .map(|entry| {
            (
                entry.path.as_path(),
                (entry.modified, entry.len, entry.kind),
            )
        })
        .collect();

    let mut changed_sources = Vec::new();
    let mut changed_assets = Vec::new();

    for (path, val) in &next_map {
        if prev_map.get(path) != Some(val) {
            match val.2 {
                WatchedKind::Source => changed_sources.push((*path).to_path_buf()),
                WatchedKind::Asset => changed_assets.push((*path).to_path_buf()),
            }
        }
    }
    for (path, val) in &prev_map {
        if !next_map.contains_key(path) {
            match val.2 {
                WatchedKind::Source => changed_sources.push((*path).to_path_buf()),
                WatchedKind::Asset => changed_assets.push((*path).to_path_buf()),
            }
        }
    }

    if !changed_sources.is_empty() {
        changed_sources.extend(changed_assets);
        ChangeClassification::Sources(changed_sources)
    } else if !changed_assets.is_empty() {
        ChangeClassification::AssetsOnly(changed_assets)
    } else {
        ChangeClassification::None
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::SystemTime;

    use super::{
        ChangeClassification, WatchedEntry, WatchedKind, classify_changes, collect_dev_fingerprint,
    };

    #[test]
    fn classifies_asset_only_vs_source_changes() {
        let base = vec![
            WatchedEntry {
                path: PathBuf::from("src/lib.rs"),
                modified: SystemTime::UNIX_EPOCH,
                len: 10,
                kind: WatchedKind::Source,
            },
            WatchedEntry {
                path: PathBuf::from("assets/macos/resources/theme.json"),
                modified: SystemTime::UNIX_EPOCH,
                len: 20,
                kind: WatchedKind::Asset,
            },
        ];
        let mut asset_edit = base.clone();
        asset_edit[1].len = 25;
        assert!(matches!(
            classify_changes(&base, &asset_edit),
            ChangeClassification::AssetsOnly(_)
        ));

        let mut source_edit = asset_edit;
        source_edit[0].len = 15;
        assert!(matches!(
            classify_changes(&base, &source_edit),
            ChangeClassification::Sources(_)
        ));
    }

    #[test]
    fn collects_both_sources_and_assets_without_target_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("src")).expect("src");
        std::fs::create_dir_all(dir.path().join("assets/resources")).expect("assets");
        std::fs::create_dir_all(dir.path().join("target")).expect("target");
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}").expect("main");
        std::fs::write(dir.path().join("assets/resources/config.json"), "{}").expect("asset");
        std::fs::write(dir.path().join("target/out.rs"), "ignored").expect("target");

        let entries =
            collect_dev_fingerprint(dir.path(), Some(&dir.path().join("assets/resources")))
                .expect("fingerprint");
        assert_eq!(entries.len(), 2);
    }
}
