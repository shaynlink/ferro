//! Isolated `WKWebView` restricted strictly to local developer server previews (`localhost`).
//!
//! No JavaScript message handlers, user scripts, or external origins are permitted.

use ferro_sdk::{FerroError, FerroErrorKind};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadOnly, define_class, msg_send};
use objc2_app_kit::NSView;
use objc2_foundation::{
    MainThreadMarker, NSObject, NSObjectProtocol, NSRect, NSString, NSURL, NSURLRequest,
};
use objc2_web_kit::{
    WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKWebView,
    WKWebViewConfiguration,
};

/// Returns `true` only when `url` targets a local loopback HTTP(S) server.
#[must_use]
pub fn is_allowed_local_preview_url(url: &str) -> bool {
    let trimmed = url.trim();
    if trimmed == "about:blank" {
        return true;
    }
    let Some(rest) = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))
    else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return false;
    }
    authority == "localhost"
        || authority.starts_with("localhost:")
        || authority == "127.0.0.1"
        || authority.starts_with("127.0.0.1:")
        || authority == "[::1]"
        || authority.starts_with("[::1]:")
}

struct DelegateIvars;

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = DelegateIvars]
    struct LocalhostNavigationDelegate;

    unsafe impl NSObjectProtocol for LocalhostNavigationDelegate {}

    unsafe impl WKNavigationDelegate for LocalhostNavigationDelegate {
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        unsafe fn decide_policy(
            &self,
            _web_view: &WKWebView,
            navigation_action: &WKNavigationAction,
            decision_handler: &block2::DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            // SAFETY: `navigation_action` is valid for the duration of this delegate callback.
            let request = unsafe { navigation_action.request() };
            let allowed = request
                .URL()
                .and_then(|url| url.absoluteString())
                .is_some_and(|raw| is_allowed_local_preview_url(&raw.to_string()));
            let policy = if allowed {
                WKNavigationActionPolicy::Allow
            } else {
                WKNavigationActionPolicy::Cancel
            };
            decision_handler.call((policy,));
        }
    }
);

impl LocalhostNavigationDelegate {
    fn new(main_thread: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(main_thread).set_ivars(DelegateIvars);
        // SAFETY: NSObject's init has its declared Objective-C signature.
        unsafe { msg_send![super(this), init] }
    }
}

/// Minimal isolated `WKWebView` for inspecting `http://localhost:<port>` developer servers.
pub struct LocalPreviewWebView {
    webview: Retained<WKWebView>,
    _delegate: Retained<LocalhostNavigationDelegate>,
}

impl LocalPreviewWebView {
    /// Creates an isolated `WKWebView` with no script message handlers or external network access.
    #[must_use]
    pub fn new(main_thread: MainThreadMarker, frame: NSRect) -> Self {
        // SAFETY: `WKWebViewConfiguration::new` runs on AppKit's main thread.
        let config = unsafe { WKWebViewConfiguration::new(main_thread) };
        // SAFETY: WKWebView allocation and initWithFrame:configuration: on main thread.
        let webview = unsafe {
            WKWebView::initWithFrame_configuration(WKWebView::alloc(main_thread), frame, &config)
        };
        let delegate = LocalhostNavigationDelegate::new(main_thread);
        // SAFETY: `delegate` implements `WKNavigationDelegate` and is retained by `Self`.
        unsafe {
            webview.setNavigationDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        }
        Self {
            webview,
            _delegate: delegate,
        }
    }

    /// Returns the underlying `NSView` for embedding in an `AppKit` layout.
    #[must_use]
    pub fn as_view(&self) -> &NSView {
        &self.webview
    }

    /// Loads a validated local loopback URL (`http://localhost:*` or `http://127.0.0.1:*`).
    ///
    /// # Errors
    ///
    /// Returns [`FerroError`] if `url` is not a local loopback URL or cannot be parsed.
    pub fn load_local_url(&self, url: &str) -> Result<(), FerroError> {
        if !is_allowed_local_preview_url(url) {
            return Err(FerroError::new(
                FerroErrorKind::Runtime,
                format!("local preview refuses non-localhost URL `{url}`"),
            ));
        }
        let ns_url = NSURL::URLWithString(&NSString::from_str(url)).ok_or_else(|| {
            FerroError::new(FerroErrorKind::Runtime, format!("invalid URL `{url}`"))
        })?;
        let request = NSURLRequest::requestWithURL(&ns_url);
        // SAFETY: `request` is a valid `NSURLRequest` on main thread.
        unsafe {
            let _ = self.webview.loadRequest(&request);
        }
        Ok(())
    }

    /// Reloads the currently loaded local preview page.
    pub fn reload(&self) {
        // SAFETY: Invoked on AppKit main thread.
        unsafe {
            let _ = self.webview.reload();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_allowed_local_preview_url;

    #[test]
    fn allows_only_loopback_urls() {
        assert!(is_allowed_local_preview_url("http://localhost:5173"));
        assert!(is_allowed_local_preview_url("http://localhost:1420/app"));
        assert!(is_allowed_local_preview_url("http://127.0.0.1:3000"));
        assert!(is_allowed_local_preview_url("https://[::1]:8443/"));
        assert!(is_allowed_local_preview_url("about:blank"));

        assert!(!is_allowed_local_preview_url("https://example.com"));
        assert!(!is_allowed_local_preview_url(
            "http://localhost.attacker.test"
        ));
        assert!(!is_allowed_local_preview_url("http://user@localhost:3000"));
        assert!(!is_allowed_local_preview_url(
            "file:///Users/test/index.html"
        ));
        assert!(!is_allowed_local_preview_url("javascript:alert(1)"));
    }
}
