//! macOS entry point.
//!
//! The `AppKit` host intentionally remains separate from the shared application crates.

use std::path::PathBuf;

use objc2::rc::autoreleasepool;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSFont, NSTextField, NSView,
    NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, ns_string};

use ferro_application::load_recent_workspaces;
use ferro_contracts::{RecentWorkspaceStore, StoreError};
use ferro_domain::Workspace;
use ferro_storage::PersistentRecentWorkspaceStore;

fn main() {
    autoreleasepool(|_| {
        let main_thread =
            MainThreadMarker::new().expect("Ferro must start on the macOS main thread");
        let application = NSApplication::sharedApplication(main_thread);
        application.setActivationPolicy(NSApplicationActivationPolicy::Regular);

        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(960.0, 640.0));
        // SAFETY: The window is allocated and initialized on AppKit's required main thread. The
        // frame and style are valid value types, and the retained result remains alive while the
        // application event loop runs.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(main_thread),
                frame,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };

        window.setTitle(ns_string!("Ferro"));
        let content = build_workspace_list(main_thread);
        window.setContentView(Some(&content));
        window.center();
        window.makeKeyAndOrderFront(None);
        application.activate();
        application.run();
    });
}

fn build_workspace_list(main_thread: MainThreadMarker) -> objc2::rc::Retained<NSView> {
    let content = NSView::initWithFrame(
        NSView::alloc(main_thread),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(960.0, 640.0)),
    );

    let title = NSTextField::labelWithString(ns_string!("Workspaces"), main_thread);
    title.setFrame(NSRect::new(
        NSPoint::new(32.0, 560.0),
        NSSize::new(896.0, 36.0),
    ));
    title.setFont(Some(&NSFont::systemFontOfSize(28.0)));
    content.addSubview(&title);

    let subtitle = NSTextField::labelWithString(
        ns_string!("Recent projects are loaded through the shared application layer."),
        main_thread,
    );
    subtitle.setFrame(NSRect::new(
        NSPoint::new(32.0, 528.0),
        NSSize::new(896.0, 24.0),
    ));
    content.addSubview(&subtitle);

    match load_workspaces() {
        Ok(model) => {
            let mut origin_y = 456.0;
            for workspace in &model.workspaces {
                add_workspace_row(&content, main_thread, workspace, origin_y);
                origin_y -= 88.0;
            }
        }
        Err(error) => add_error(&content, main_thread, &error),
    }

    content
}

fn load_workspaces() -> Result<ferro_application::RecentWorkspaces, StoreError> {
    let mut store = PersistentRecentWorkspaceStore::open(recent_workspaces_database_path()?)?;
    let model = load_recent_workspaces(&store)?;
    if !model.workspaces.is_empty() {
        return Ok(model);
    }

    let root = std::env::current_dir().map_err(|error| {
        StoreError::new(format!("could not locate the current workspace: {error}"))
    })?;
    let workspace = Workspace::new("Ferro", root).map_err(|error| {
        StoreError::new(format!("could not create the initial workspace: {error}"))
    })?;
    store.remember_workspace(&workspace)?;
    load_recent_workspaces(&store)
}

fn recent_workspaces_database_path() -> Result<PathBuf, StoreError> {
    let home = std::env::var_os("HOME")
        .ok_or_else(|| StoreError::new("could not determine the home directory"))?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("Ferro")
        .join("catalog.lilia"))
}

fn add_workspace_row(
    content: &NSView,
    main_thread: MainThreadMarker,
    workspace: &Workspace,
    origin_y: f64,
) {
    let name = NSString::from_str(workspace.name());
    let name_label = NSTextField::labelWithString(&name, main_thread);
    name_label.setFrame(NSRect::new(
        NSPoint::new(40.0, origin_y + 26.0),
        NSSize::new(880.0, 24.0),
    ));
    name_label.setFont(Some(&NSFont::systemFontOfSize(18.0)));
    content.addSubview(&name_label);

    let root = NSString::from_str(&workspace.root().display().to_string());
    let root_label = NSTextField::labelWithString(&root, main_thread);
    root_label.setFrame(NSRect::new(
        NSPoint::new(40.0, origin_y),
        NSSize::new(880.0, 20.0),
    ));
    content.addSubview(&root_label);
}

fn add_error(content: &NSView, main_thread: MainThreadMarker, error: &StoreError) {
    let text = NSString::from_str(&format!("Could not load workspaces: {error}"));
    let label = NSTextField::labelWithString(&text, main_thread);
    label.setFrame(NSRect::new(
        NSPoint::new(32.0, 456.0),
        NSSize::new(896.0, 24.0),
    ));
    content.addSubview(&label);
}
