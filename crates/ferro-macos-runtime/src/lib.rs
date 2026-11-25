//! Native `AppKit` development runtime for Ferro applications.
//!
//! This crate is deliberately macOS-specific. It consumes the portable SDK contract but does not
//! expose `AppKit` types back into shared application code.

use ferro_sdk::{Application, ApplicationDescriptor};
use objc2::rc::autoreleasepool;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSFont, NSTextField, NSView,
    NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, ns_string};

/// Starts a native macOS development window for `A`.
///
/// The function runs `AppKit`'s event loop and therefore does not return until the application
/// terminates.
///
/// # Panics
///
/// Panics when called outside macOS's main thread.
pub fn run<A: Application>() {
    autoreleasepool(|_| {
        let main_thread = MainThreadMarker::new()
            .expect("a Ferro macOS application must start on the main thread");
        let application = NSApplication::sharedApplication(main_thread);
        application.setActivationPolicy(NSApplicationActivationPolicy::Regular);

        let descriptor = A::descriptor();
        let window = build_window(main_thread, &descriptor);
        window.center();
        window.makeKeyAndOrderFront(None);
        application.activate();
        application.run();
    });
}

fn build_window(
    main_thread: MainThreadMarker,
    descriptor: &ApplicationDescriptor,
) -> objc2::rc::Retained<NSWindow> {
    let style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable
        | NSWindowStyleMask::Resizable;
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(720.0, 420.0));
    // SAFETY: The window is allocated and initialized on AppKit's required main thread. The frame
    // and style are valid value types, and the retained result stays alive through the event loop.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(main_thread),
            frame,
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    };

    let title = NSString::from_str(descriptor.name());
    window.setTitle(&title);
    let content = build_content(main_thread, descriptor);
    window.setContentView(Some(&content));
    window
}

fn build_content(
    main_thread: MainThreadMarker,
    descriptor: &ApplicationDescriptor,
) -> objc2::rc::Retained<NSView> {
    let content = NSView::initWithFrame(
        NSView::alloc(main_thread),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(720.0, 420.0)),
    );

    let title = NSString::from_str(descriptor.name());
    let title_label = NSTextField::labelWithString(&title, main_thread);
    title_label.setFrame(NSRect::new(
        NSPoint::new(32.0, 300.0),
        NSSize::new(656.0, 36.0),
    ));
    title_label.setFont(Some(&NSFont::systemFontOfSize(28.0)));
    content.addSubview(&title_label);

    let identifier = NSString::from_str(&format!("Application ID: {}", descriptor.id()));
    let identifier_label = NSTextField::labelWithString(&identifier, main_thread);
    identifier_label.setFrame(NSRect::new(
        NSPoint::new(32.0, 264.0),
        NSSize::new(656.0, 24.0),
    ));
    content.addSubview(&identifier_label);

    let message = NSTextField::labelWithString(
        ns_string!("Running with the Ferro macOS development runtime."),
        main_thread,
    );
    message.setFrame(NSRect::new(
        NSPoint::new(32.0, 224.0),
        NSSize::new(656.0, 24.0),
    ));
    content.addSubview(&message);

    content
}

#[cfg(test)]
fn application_title<A: Application>() -> String {
    A::descriptor().name().to_owned()
}

#[cfg(test)]
mod tests {
    use ferro_sdk::{Application, ApplicationDescriptor};

    use super::application_title;

    struct Atlas;

    impl Application for Atlas {
        fn descriptor() -> ApplicationDescriptor {
            ApplicationDescriptor::new("Atlas", "dev.ferro.atlas")
        }
    }

    #[test]
    fn reads_the_application_title_from_the_portable_contract() {
        assert_eq!(application_title::<Atlas>(), "Atlas");
    }
}
