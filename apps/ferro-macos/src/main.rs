//! macOS entry point.
//!
//! The `AppKit` host intentionally remains separate from the shared application crates.

use objc2::rc::autoreleasepool;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, ns_string};

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
        window.center();
        window.makeKeyAndOrderFront(None);
        application.activate();
        application.run();
    });
}
