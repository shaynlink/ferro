//! Test-only adapters, UI tree models, accessibility audits, and automation harnesses.

use std::fmt;
use std::fmt::Write as _;
use std::time::Instant;

use ferro_contracts::{RecentWorkspaceStore, StoreError};
use ferro_domain::Workspace;
use ferro_sdk::{Application, Lifecycle, WindowLifecycle};
use ferro_state::{StateTransition, TimeTravelStore};
use serde::{Deserialize, Serialize};

/// In-memory recent-workspace store for deterministic tests.
#[derive(Clone, Debug, Default)]
pub struct MemoryRecentWorkspaceStore {
    workspaces: Vec<Workspace>,
}

impl MemoryRecentWorkspaceStore {
    /// Builds a store with a known most-recent-first sequence.
    #[must_use]
    pub fn with_workspaces(workspaces: Vec<Workspace>) -> Self {
        Self { workspaces }
    }
}

impl RecentWorkspaceStore for MemoryRecentWorkspaceStore {
    fn recent_workspaces(&self) -> Result<Vec<Workspace>, StoreError> {
        Ok(self.workspaces.clone())
    }

    fn remember_workspace(&mut self, workspace: &Workspace) -> Result<(), StoreError> {
        self.workspaces
            .retain(|item| item.root() != workspace.root());
        self.workspaces.insert(0, workspace.clone());
        Ok(())
    }
}

/// Headless test driver for deterministic verification of any [`Application`] reducer.
pub struct TestApp<A: Application> {
    store: TimeTravelStore<A>,
    recorded_events: Vec<A::Event>,
    recorded_effects: Vec<A::Effect>,
    lifecycles: Vec<Lifecycle>,
    window_lifecycles: Vec<WindowLifecycle>,
}

impl<A: Application> TestApp<A>
where
    A::State: Clone,
    A::Event: Clone + fmt::Debug,
    A::Effect: Clone + fmt::Debug,
{
    /// Starts a headless test session using `A::initial_state()`.
    #[must_use]
    pub fn launch() -> Self {
        Self {
            store: TimeTravelStore::new(512),
            recorded_events: Vec::new(),
            recorded_effects: Vec::new(),
            lifecycles: Vec::new(),
            window_lifecycles: Vec::new(),
        }
    }

    /// Returns the active application state.
    #[must_use]
    pub const fn state(&self) -> &A::State {
        self.store.state()
    }

    /// Sends one event through the reducer and records all emitted effects.
    pub fn send(&mut self, event: A::Event) -> Vec<A::Effect> {
        self.recorded_events.push(event.clone());
        let effects = self.store.dispatch(event);
        self.recorded_effects.extend(effects.iter().cloned());
        effects
    }

    /// Sends a sequence of events in order, returning every emitted effect.
    pub fn send_all<I>(&mut self, events: I) -> Vec<A::Effect>
    where
        I: IntoIterator<Item = A::Event>,
    {
        let mut all_effects = Vec::new();
        for event in events {
            all_effects.extend(self.send(event));
        }
        all_effects
    }

    /// Records a [`Lifecycle`] transition and maps it with `mapper` into an optional event.
    pub fn trigger_lifecycle(
        &mut self,
        lifecycle: Lifecycle,
        mapper: impl FnOnce(Lifecycle) -> Option<A::Event>,
    ) -> Vec<A::Effect> {
        self.lifecycles.push(lifecycle);
        mapper(lifecycle).map_or_else(Vec::new, |event| self.send(event))
    }

    /// Records a [`WindowLifecycle`] notification and maps it with `mapper` into an optional event.
    pub fn trigger_window_lifecycle(
        &mut self,
        lifecycle: WindowLifecycle,
        mapper: impl FnOnce(WindowLifecycle) -> Option<A::Event>,
    ) -> Vec<A::Effect> {
        self.window_lifecycles.push(lifecycle.clone());
        mapper(lifecycle).map_or_else(Vec::new, |event| self.send(event))
    }

    /// Returns all dispatched events in chronological order.
    #[must_use]
    pub fn events(&self) -> &[A::Event] {
        &self.recorded_events
    }

    /// Returns all emitted effects in chronological order.
    #[must_use]
    pub fn effects(&self) -> &[A::Effect] {
        &self.recorded_effects
    }

    /// Returns all recorded lifecycle transitions.
    #[must_use]
    pub fn lifecycles(&self) -> &[Lifecycle] {
        &self.lifecycles
    }

    /// Returns all recorded window lifecycle transitions.
    #[must_use]
    pub fn window_lifecycles(&self) -> &[WindowLifecycle] {
        &self.window_lifecycles
    }

    /// Returns the state transition timeline.
    #[must_use]
    pub fn transitions(&self) -> &[StateTransition<A::State>] {
        self.store.history()
    }

    /// Rewinds state to transition index `index` for time-travel verification.
    pub fn rewind_to(&mut self, index: usize) -> bool {
        self.store.jump_to(index)
    }

    /// Clears recorded events and effects while preserving current state.
    pub fn clear_logs(&mut self) {
        self.recorded_events.clear();
        self.recorded_effects.clear();
    }
}

/// Semantic role of a native or simulated UI node.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiRole {
    /// Application window container.
    Window,
    /// Generic view / layout container.
    View,
    /// Clickable button.
    Button,
    /// Editable or read-only text field.
    TextField,
    /// Static text label.
    Label,
    /// Toggle or checkbox control.
    Checkbox,
    /// Scroll container.
    ScrollView,
    /// List or table view.
    List,
    /// Embedded web preview view.
    WebView,
    /// Native menu item.
    MenuItem,
    /// Any platform-specific role outside standard categories.
    Custom(String),
}

impl UiRole {
    /// Returns whether nodes of this role typically require an accessible label or title.
    #[must_use]
    pub const fn requires_accessible_label(&self) -> bool {
        matches!(
            self,
            Self::Button | Self::TextField | Self::Checkbox | Self::MenuItem
        )
    }

    /// Returns whether this role represents an interactive user control.
    #[must_use]
    pub const fn is_interactive(&self) -> bool {
        matches!(
            self,
            Self::Button | Self::TextField | Self::Checkbox | Self::List | Self::MenuItem
        )
    }
}

/// Logical point rectangle of a UI element.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UiRect {
    /// Horizontal origin in logical points.
    pub x: f64,
    /// Vertical origin in logical points.
    pub y: f64,
    /// Width in logical points.
    pub width: f64,
    /// Height in logical points.
    pub height: f64,
}

impl UiRect {
    /// Creates a new rectangle.
    #[must_use]
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Returns whether this rectangle has positive non-zero area.
    #[must_use]
    pub fn has_positive_area(&self) -> bool {
        self.width > 0.0 && self.height > 0.0
    }
}

/// Portable snapshot of a node in a native or simulated UI hierarchy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UiNode {
    /// Stable developer or accessibility identifier (`accessibilityIdentifier`).
    pub id: Option<String>,
    /// Native platform class name (e.g. `NSButton`, `NSTextField`).
    pub class_name: String,
    /// Semantic element role.
    pub role: UiRole,
    /// Visible title or accessibility label.
    pub title: Option<String>,
    /// Current textual value (if applicable).
    pub value: Option<String>,
    /// Frame in parent/window logical points.
    pub frame: UiRect,
    /// Whether the element accepts interaction.
    pub enabled: bool,
    /// Whether the element is currently visible.
    pub visible: bool,
    /// Whether the element currently holds keyboard focus.
    pub focused: bool,
    /// Child elements in display order.
    #[serde(default)]
    pub children: Vec<UiNode>,
}

impl UiNode {
    /// Creates a basic visible, enabled [`UiNode`].
    #[must_use]
    pub fn new(class_name: impl Into<String>, role: UiRole, frame: UiRect) -> Self {
        Self {
            id: None,
            class_name: class_name.into(),
            role,
            title: None,
            value: None,
            frame,
            enabled: true,
            visible: true,
            focused: false,
            children: Vec::new(),
        }
    }

    /// Attaches a stable accessibility/test identifier.
    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Attaches a title or accessible label.
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Attaches a current textual value.
    #[must_use]
    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Sets visibility.
    #[must_use]
    pub const fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Sets enabled state.
    #[must_use]
    pub const fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Appends a child node.
    #[must_use]
    pub fn with_child(mut self, child: Self) -> Self {
        self.children.push(child);
        self
    }

    /// Returns the preferred human-readable label (`title` or `value` or `id`).
    #[must_use]
    pub fn display_label(&self) -> Option<&str> {
        self.title
            .as_deref()
            .filter(|text| !text.trim().is_empty())
            .or_else(|| self.value.as_deref().filter(|text| !text.trim().is_empty()))
            .or_else(|| self.id.as_deref().filter(|text| !text.trim().is_empty()))
    }

    /// Visits this node and all descendants depth-first.
    pub fn visit<'a>(&'a self, visitor: &mut impl FnMut(&'a Self)) {
        visitor(self);
        for child in &self.children {
            child.visit(visitor);
        }
    }
}

/// Accessibility diagnostic discovered during a UI tree audit.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AccessibilityIssue {
    /// Severity level (`error` or `warning`).
    pub severity: String,
    /// Rule identifier (e.g. `missing-label`, `zero-frame`, `duplicate-id`).
    pub rule: String,
    /// Affected element identifier or class path.
    pub target: String,
    /// Human-readable remediation message.
    pub message: String,
}

/// Complete UI hierarchy captured from one application window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UiTree {
    /// Application name.
    pub app_name: String,
    /// Window identifier.
    pub window_id: String,
    /// Root node (typically the window or content view).
    pub root: UiNode,
}

impl UiTree {
    /// Counts total nodes in the hierarchy.
    #[must_use]
    pub fn node_count(&self) -> usize {
        let mut count = 0;
        self.root.visit(&mut |_| count += 1);
        count
    }

    /// Finds the first node matching an exact identifier (`id`).
    #[must_use]
    pub fn find_by_id(&self, id: &str) -> Option<&UiNode> {
        find_node(&self.root, &|node| node.id.as_deref() == Some(id))
    }

    /// Finds the first node whose `title` or `value` contains `needle`.
    #[must_use]
    pub fn find_by_text(&self, needle: &str) -> Option<&UiNode> {
        find_node(&self.root, &|node| {
            node.title
                .as_deref()
                .is_some_and(|text| text.contains(needle))
                || node
                    .value
                    .as_deref()
                    .is_some_and(|text| text.contains(needle))
        })
    }

    /// Returns all nodes matching `role`.
    #[must_use]
    pub fn find_by_role(&self, role: &UiRole) -> Vec<&UiNode> {
        let mut matches = Vec::new();
        self.root.visit(&mut |node| {
            if &node.role == role {
                matches.push(node);
            }
        });
        matches
    }

    /// Audits the UI hierarchy for common accessibility and layout defects.
    #[must_use]
    pub fn audit_accessibility(&self) -> Vec<AccessibilityIssue> {
        let mut issues = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        self.root.visit(&mut |node| {
            let target = node
                .id
                .clone()
                .unwrap_or_else(|| format!("{}({:?})", node.class_name, node.role));
            if let Some(id) = &node.id
                && !seen_ids.insert(id.clone())
            {
                issues.push(AccessibilityIssue {
                    severity: "warning".to_owned(),
                    rule: "duplicate-id".to_owned(),
                    target: target.clone(),
                    message: format!("duplicate accessibility/UI identifier `{id}`"),
                });
            }
            if node.visible
                && node.role.requires_accessible_label()
                && node.display_label().is_none()
            {
                issues.push(AccessibilityIssue {
                    severity: "error".to_owned(),
                    rule: "missing-label".to_owned(),
                    target: target.clone(),
                    message: format!(
                        "interactive {:?} control `{}` has no title, value, or accessibility id",
                        node.role, node.class_name
                    ),
                });
            }
            if node.visible && node.role.is_interactive() && !node.frame.has_positive_area() {
                issues.push(AccessibilityIssue {
                    severity: "warning".to_owned(),
                    rule: "zero-frame".to_owned(),
                    target: target.clone(),
                    message: format!(
                        "visible interactive control `{}` has non-positive frame {}x{}",
                        node.class_name, node.frame.width, node.frame.height
                    ),
                });
            }
            if node.visible
                && matches!(node.role, UiRole::Button)
                && node.frame.has_positive_area()
                && (node.frame.width < 20.0 || node.frame.height < 20.0)
            {
                issues.push(AccessibilityIssue {
                    severity: "warning".to_owned(),
                    rule: "small-hit-target".to_owned(),
                    target,
                    message: format!(
                        "button `{}` hit target ({:.1}x{:.1}) is smaller than 20x20pt",
                        node.class_name, node.frame.width, node.frame.height
                    ),
                });
            }
        });
        issues
    }
}

fn find_node<'a>(node: &'a UiNode, predicate: &impl Fn(&UiNode) -> bool) -> Option<&'a UiNode> {
    if predicate(node) {
        return Some(node);
    }
    for child in &node.children {
        if let Some(found) = find_node(child, predicate) {
            return Some(found);
        }
    }
    None
}

/// Selector used by UI automation steps to address an element.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiSelector {
    /// Matches [`UiNode::id`] exactly.
    Id(String),
    /// Matches substring in [`UiNode::title`] or [`UiNode::value`].
    Text(String),
}

impl UiSelector {
    /// Resolves this selector against a [`UiTree`].
    #[must_use]
    pub fn resolve<'a>(&self, tree: &'a UiTree) -> Option<&'a UiNode> {
        match self {
            Self::Id(id) => tree.find_by_id(id),
            Self::Text(text) => tree.find_by_text(text),
        }
    }
}

/// Action executed by the UI automation runner.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum UiAction {
    /// Verifies that an element exists in the UI tree.
    AssertExists {
        /// Target selector.
        selector: UiSelector,
    },
    /// Verifies that an element is visible.
    AssertVisible {
        /// Target selector.
        selector: UiSelector,
    },
    /// Verifies that the target element's title or value contains `expected`.
    AssertTextContains {
        /// Target selector.
        selector: UiSelector,
        /// Expected substring.
        expected: String,
    },
    /// Triggers a native click/activation on a control.
    Click {
        /// Target selector.
        selector: UiSelector,
    },
    /// Replaces text inside an editable control.
    SetText {
        /// Target selector.
        selector: UiSelector,
        /// Replacement text value.
        text: String,
    },
    /// Resizes a window to the given logical dimensions.
    ResizeWindow {
        /// Target window id (`main` by default).
        window_id: String,
        /// Target width in logical points.
        width: f64,
        /// Target height in logical points.
        height: f64,
    },
    /// Runs an accessibility audit and fails if any `error`-severity issues exist.
    AuditAccessibility {
        /// Whether `warning` issues should also fail the step.
        strict: bool,
    },
    /// Waits for `duration_ms` milliseconds.
    Wait {
        /// Delay duration in milliseconds.
        duration_ms: u64,
    },
}

/// Named UI automation scenario composed of ordered steps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutomationScenario {
    /// Scenario name shown in test and CI reports.
    pub name: String,
    /// Ordered automation actions.
    pub steps: Vec<UiAction>,
}

/// Outcome of a single executed automation step.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationStepResult {
    /// Zero-based step index.
    pub index: usize,
    /// Step description.
    pub description: String,
    /// Whether the step passed.
    pub passed: bool,
    /// Execution duration in milliseconds.
    pub duration_ms: u64,
    /// Diagnostic detail when failed or informational note.
    pub detail: Option<String>,
}

/// Aggregate report produced by executing an [`AutomationScenario`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AutomationReport {
    /// Scenario name.
    pub scenario: String,
    /// Overall pass/fail status.
    pub passed: bool,
    /// Total duration in milliseconds.
    pub duration_ms: u64,
    /// Per-step outcomes.
    pub steps: Vec<AutomationStepResult>,
    /// Accessibility diagnostics collected during the run.
    pub accessibility_issues: Vec<AccessibilityIssue>,
}

impl AutomationReport {
    /// Renders this report as standard `JUnit` XML suitable for CI systems.
    #[must_use]
    pub fn to_junit_xml(&self) -> String {
        let failures = self.steps.iter().filter(|step| !step.passed).count();
        #[allow(
            clippy::cast_precision_loss,
            reason = "test durations in ms fit cleanly in f64 seconds for JUnit"
        )]
        let total_secs = (self.duration_ms as f64) / 1000.0;
        let mut xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" time=\"{total_secs:.3}\">\n",
            xml_escape(&self.scenario),
            self.steps.len(),
            failures
        );
        for step in &self.steps {
            #[allow(
                clippy::cast_precision_loss,
                reason = "step durations in ms fit cleanly in f64 seconds for JUnit"
            )]
            let secs = (step.duration_ms as f64) / 1000.0;
            let _ = write!(
                xml,
                "  <testcase name=\"step-{}: {}\" time=\"{secs:.3}\"",
                step.index,
                xml_escape(&step.description)
            );
            if step.passed {
                xml.push_str(" />\n");
            } else {
                let message = step.detail.as_deref().unwrap_or("automation step failed");
                let _ = write!(
                    xml,
                    ">\n    <failure message=\"{}\" />\n  </testcase>\n",
                    xml_escape(message)
                );
            }
        }
        xml.push_str("</testsuite>\n");
        xml
    }
}

/// Evaluates read-only/structural assertions and accessibility steps of an [`AutomationScenario`]
/// directly against a captured [`UiTree`] (with optional mutable simulator handler for mutations).
#[must_use]
pub fn evaluate_scenario_on_tree(
    scenario: &AutomationScenario,
    tree: &mut UiTree,
    mut on_mutation: impl FnMut(&UiAction, &mut UiTree) -> Result<String, String>,
) -> AutomationReport {
    let total_start = Instant::now();
    let mut step_results = Vec::with_capacity(scenario.steps.len());
    let mut accessibility_issues = Vec::new();
    let mut all_passed = true;

    for (index, action) in scenario.steps.iter().enumerate() {
        let step_start = Instant::now();
        let description = describe_action(action);
        let outcome: Result<Option<String>, String> = match action {
            UiAction::AssertExists { selector } => {
                if selector.resolve(tree).is_some() {
                    Ok(None)
                } else {
                    Err(format!("element matching {selector:?} was not found"))
                }
            }
            UiAction::AssertVisible { selector } => match selector.resolve(tree) {
                Some(node) if node.visible => Ok(None),
                Some(node) => Err(format!(
                    "element `{}` exists but is hidden",
                    node.class_name
                )),
                None => Err(format!("element matching {selector:?} was not found")),
            },
            UiAction::AssertTextContains { selector, expected } => match selector.resolve(tree) {
                Some(node) => {
                    let matches = node
                        .title
                        .as_deref()
                        .is_some_and(|text| text.contains(expected))
                        || node
                            .value
                            .as_deref()
                            .is_some_and(|text| text.contains(expected));
                    if matches {
                        Ok(None)
                    } else {
                        Err(format!(
                            "element `{}` text ({:?} / {:?}) did not contain `{expected}`",
                            node.class_name, node.title, node.value
                        ))
                    }
                }
                None => Err(format!("element matching {selector:?} was not found")),
            },
            UiAction::AuditAccessibility { strict } => {
                let found = tree.audit_accessibility();
                let failing = found
                    .iter()
                    .filter(|issue| *strict || issue.severity == "error")
                    .count();
                accessibility_issues = found;
                if failing == 0 {
                    Ok(Some(format!(
                        "{} accessibility note(s), 0 blocking",
                        accessibility_issues.len()
                    )))
                } else {
                    Err(format!("{failing} blocking accessibility issue(s) found"))
                }
            }
            UiAction::Wait { duration_ms } => {
                std::thread::sleep(std::time::Duration::from_millis(*duration_ms));
                Ok(None)
            }
            UiAction::Click { .. } | UiAction::SetText { .. } | UiAction::ResizeWindow { .. } => {
                on_mutation(action, tree).map(Some)
            }
        };

        let elapsed_ms = u64::try_from(step_start.elapsed().as_millis()).unwrap_or(u64::MAX);
        match outcome {
            Ok(detail) => step_results.push(AutomationStepResult {
                index,
                description,
                passed: true,
                duration_ms: elapsed_ms,
                detail,
            }),
            Err(message) => {
                all_passed = false;
                step_results.push(AutomationStepResult {
                    index,
                    description,
                    passed: false,
                    duration_ms: elapsed_ms,
                    detail: Some(message),
                });
            }
        }
    }

    AutomationReport {
        scenario: scenario.name.clone(),
        passed: all_passed,
        duration_ms: u64::try_from(total_start.elapsed().as_millis()).unwrap_or(u64::MAX),
        steps: step_results,
        accessibility_issues,
    }
}

fn describe_action(action: &UiAction) -> String {
    match action {
        UiAction::AssertExists { selector } => format!("assert exists {selector:?}"),
        UiAction::AssertVisible { selector } => format!("assert visible {selector:?}"),
        UiAction::AssertTextContains { selector, expected } => {
            format!("assert text of {selector:?} contains `{expected}`")
        }
        UiAction::Click { selector } => format!("click {selector:?}"),
        UiAction::SetText { selector, text } => format!("set text on {selector:?} -> `{text}`"),
        UiAction::ResizeWindow {
            window_id,
            width,
            height,
        } => format!("resize window `{window_id}` to {width}x{height}"),
        UiAction::AuditAccessibility { strict } => {
            format!("audit accessibility (strict={strict})")
        }
        UiAction::Wait { duration_ms } => format!("wait {duration_ms}ms"),
    }
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use ferro_sdk::{Application, ApplicationDescriptor, Lifecycle};

    use super::{
        AutomationScenario, TestApp, UiAction, UiNode, UiRect, UiRole, UiSelector, UiTree,
        evaluate_scenario_on_tree,
    };

    struct DemoApp;

    impl Application for DemoApp {
        type State = String;
        type Event = String;
        type Effect = &'static str;

        fn descriptor() -> ApplicationDescriptor {
            ApplicationDescriptor::new("Demo", "dev.ferro.demo")
        }

        fn initial_state() -> Self::State {
            "init".to_owned()
        }

        fn reduce(state: &mut Self::State, event: Self::Event) -> Vec<Self::Effect> {
            *state = event;
            vec!["render"]
        }
    }

    #[test]
    fn test_app_drives_events_lifecycle_and_time_travel() {
        let mut app = TestApp::<DemoApp>::launch();
        assert_eq!(app.state(), "init");

        app.trigger_lifecycle(Lifecycle::Launched, |lc| {
            (lc == Lifecycle::Launched).then(|| "launched".to_owned())
        });
        assert_eq!(app.state(), "launched");

        app.send("updated".to_owned());
        assert_eq!(app.state(), "updated");
        assert_eq!(app.events(), ["launched", "updated"]);
        assert_eq!(app.effects(), ["render", "render"]);

        assert!(app.rewind_to(1));
        assert_eq!(app.state(), "launched");
    }

    #[test]
    fn ui_tree_audits_accessibility_and_executes_automation_scenario() {
        let mut tree = UiTree {
            app_name: "Atlas".to_owned(),
            window_id: "main".to_owned(),
            root: UiNode::new(
                "NSWindow",
                UiRole::Window,
                UiRect::new(0.0, 0.0, 800.0, 520.0),
            )
            .with_id("main-window")
            .with_title("Atlas")
            .with_child(
                UiNode::new(
                    "NSTextField",
                    UiRole::Label,
                    UiRect::new(32.0, 340.0, 200.0, 24.0),
                )
                .with_id("status-label")
                .with_value("Ready"),
            )
            .with_child(
                UiNode::new(
                    "NSButton",
                    UiRole::Button,
                    UiRect::new(32.0, 280.0, 120.0, 32.0),
                )
                .with_id("open-btn")
                .with_title("Open File"),
            ),
        };

        let scenario = AutomationScenario {
            name: "smoke-ui".to_owned(),
            steps: vec![
                UiAction::AssertExists {
                    selector: UiSelector::Id("open-btn".to_owned()),
                },
                UiAction::AssertVisible {
                    selector: UiSelector::Text("Ready".to_owned()),
                },
                UiAction::AssertTextContains {
                    selector: UiSelector::Id("status-label".to_owned()),
                    expected: "Ready".to_owned(),
                },
                UiAction::Click {
                    selector: UiSelector::Id("open-btn".to_owned()),
                },
                UiAction::AuditAccessibility { strict: true },
            ],
        };

        let report = evaluate_scenario_on_tree(&scenario, &mut tree, |action, _| match action {
            UiAction::Click { .. } => Ok("clicked".to_owned()),
            _ => Err("unsupported".to_owned()),
        });

        assert!(report.passed);
        assert_eq!(report.accessibility_issues, Vec::new());
        assert!(
            report
                .to_junit_xml()
                .contains("<testsuite name=\"smoke-ui\"")
        );
    }

    #[test]
    fn detects_missing_label_and_small_hit_target_accessibility_issues() {
        let tree = UiTree {
            app_name: "Atlas".to_owned(),
            window_id: "main".to_owned(),
            root: UiNode::new(
                "NSWindow",
                UiRole::Window,
                UiRect::new(0.0, 0.0, 800.0, 520.0),
            )
            .with_child(UiNode::new(
                "NSButton",
                UiRole::Button,
                UiRect::new(10.0, 10.0, 14.0, 14.0),
            )),
        };

        let issues = tree.audit_accessibility();
        assert!(issues.iter().any(|issue| issue.rule == "missing-label"));
        assert!(issues.iter().any(|issue| issue.rule == "small-hit-target"));
    }
}
