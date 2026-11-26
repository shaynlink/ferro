//! CPU/span/frame profiling, Chrome Trace export, macOS `sample` parsing, and pure-Rust SVG flamegraphs.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// One completed timing span recorded by [`Profiler`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProfileSpan {
    /// Span operation name (e.g. `reduce`, `render`, `build`).
    pub name: String,
    /// Logical category (e.g. `state`, `ui`, `effect`, `io`).
    pub category: String,
    /// Start offset in microseconds relative to profiler origin.
    pub start_us: u64,
    /// Duration in microseconds.
    pub duration_us: u64,
    /// Thread identifier label.
    pub thread_id: u64,
}

/// Aggregated statistics for a single span name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpanStats {
    /// Span name.
    pub name: String,
    /// Category of the first recorded occurrence.
    pub category: String,
    /// Total invocation count.
    pub count: usize,
    /// Total cumulative time in microseconds.
    pub total_us: u64,
    /// Mean duration in microseconds.
    pub avg_us: f64,
    /// Minimum duration in microseconds.
    pub min_us: u64,
    /// 95th percentile duration in microseconds.
    pub p95_us: u64,
    /// Maximum duration in microseconds.
    pub max_us: u64,
}

/// Frame-budget summary for UI render cycles.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrameSummary {
    /// Number of recorded frames.
    pub total_frames: usize,
    /// Target frame budget in microseconds (e.g. `16_667` for 60 FPS or `8_333` for 120 FPS).
    pub budget_us: u64,
    /// Count of frames exceeding [`Self::budget_us`].
    pub dropped_frames: usize,
    /// Average frame duration in microseconds.
    pub avg_us: f64,
    /// 95th percentile frame duration in microseconds.
    pub p95_us: u64,
    /// Maximum frame duration in microseconds.
    pub max_us: u64,
    /// Estimated effective FPS derived from average frame time.
    pub estimated_fps: f64,
}

/// Complete profiler report ready for CLI display or JSON serialization.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileSummary {
    /// Aggregated span timings sorted by descending `total_us`.
    pub spans: Vec<SpanStats>,
    /// UI frame-budget statistics.
    pub frames: FrameSummary,
    /// Total retained span records.
    pub retained_span_count: usize,
}

/// Thread-safe-compatible in-process profiler collecting spans and frame budgets.
#[derive(Clone, Debug)]
pub struct Profiler {
    origin: Instant,
    max_spans: usize,
    max_frames: usize,
    frame_budget_us: u64,
    spans: Vec<ProfileSpan>,
    frame_durations_us: Vec<u64>,
}

impl Profiler {
    /// Creates a profiler with bounded span and frame buffers.
    #[must_use]
    pub fn new(max_spans: usize, max_frames: usize, target_fps: u32) -> Self {
        let fps = u64::from(target_fps.max(1));
        Self {
            origin: Instant::now(),
            max_spans: max_spans.max(1),
            max_frames: max_frames.max(1),
            frame_budget_us: 1_000_000 / fps,
            spans: Vec::new(),
            frame_durations_us: Vec::new(),
        }
    }

    /// Measures and records a synchronous closure as a named span.
    pub fn measure<T>(&mut self, name: &str, category: &str, f: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let out = f();
        let elapsed = start.elapsed();
        self.record_span_with_start(name, category, start, elapsed);
        out
    }

    /// Records a span with an explicit start instant and duration.
    pub fn record_span_with_start(
        &mut self,
        name: &str,
        category: &str,
        start: Instant,
        duration: Duration,
    ) {
        let start_us = u64::try_from(start.saturating_duration_since(self.origin).as_micros())
            .unwrap_or(u64::MAX);
        let duration_us = u64::try_from(duration.as_micros()).unwrap_or(u64::MAX);
        if self.spans.len() >= self.max_spans {
            self.spans.remove(0);
        }
        self.spans.push(ProfileSpan {
            name: name.to_owned(),
            category: category.to_owned(),
            start_us,
            duration_us,
            thread_id: 1,
        });
    }

    /// Records one completed UI render/frame duration.
    pub fn record_frame(&mut self, duration: Duration) {
        let duration_us = u64::try_from(duration.as_micros()).unwrap_or(u64::MAX);
        if self.frame_durations_us.len() >= self.max_frames {
            self.frame_durations_us.remove(0);
        }
        self.frame_durations_us.push(duration_us);
    }

    /// Returns all retained spans.
    #[must_use]
    pub fn spans(&self) -> &[ProfileSpan] {
        &self.spans
    }

    /// Computes aggregate statistics across all retained spans and frames.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "microsecond aggregates and counts are formatted for human diagnostics"
    )]
    pub fn summarize(&self) -> ProfileSummary {
        let mut grouped: BTreeMap<&str, (&str, Vec<u64>)> = BTreeMap::new();
        for span in &self.spans {
            let entry = grouped
                .entry(&span.name)
                .or_insert_with(|| (&span.category, Vec::new()));
            entry.1.push(span.duration_us);
        }

        let mut spans: Vec<SpanStats> = grouped
            .into_iter()
            .map(|(name, (category, mut durations))| {
                durations.sort_unstable();
                let count = durations.len();
                let total_us: u64 = durations.iter().sum();
                let avg_us = (total_us as f64) / (count as f64);
                let min_us = *durations.first().unwrap_or(&0);
                let max_us = *durations.last().unwrap_or(&0);
                let p95_idx = ((count.saturating_sub(1)) * 95) / 100;
                let p95_us = *durations.get(p95_idx).unwrap_or(&max_us);
                SpanStats {
                    name: name.to_owned(),
                    category: category.to_owned(),
                    count,
                    total_us,
                    avg_us,
                    min_us,
                    p95_us,
                    max_us,
                }
            })
            .collect();
        spans.sort_by_key(|stat| std::cmp::Reverse(stat.total_us));

        let mut frame_sorted = self.frame_durations_us.clone();
        frame_sorted.sort_unstable();
        let total_frames = frame_sorted.len();
        let dropped_frames = frame_sorted
            .iter()
            .filter(|&&dur| dur > self.frame_budget_us)
            .count();
        let frame_total_us: u64 = frame_sorted.iter().sum();
        let avg_us = if total_frames == 0 {
            0.0
        } else {
            (frame_total_us as f64) / (total_frames as f64)
        };
        let max_us = *frame_sorted.last().unwrap_or(&0);
        let p95_idx = ((total_frames.saturating_sub(1)) * 95) / 100;
        let p95_us = *frame_sorted.get(p95_idx).unwrap_or(&max_us);
        let estimated_fps = if avg_us > 0.0 {
            1_000_000.0 / avg_us
        } else {
            0.0
        };

        ProfileSummary {
            spans,
            frames: FrameSummary {
                total_frames,
                budget_us: self.frame_budget_us,
                dropped_frames,
                avg_us,
                p95_us,
                max_us,
                estimated_fps,
            },
            retained_span_count: self.spans.len(),
        }
    }

    /// Exports retained spans in Chrome Trace Event JSON format (`chrome://tracing` / Perfetto).
    #[must_use]
    pub fn to_chrome_trace_json(&self) -> String {
        let events: Vec<serde_json::Value> = self
            .spans
            .iter()
            .map(|span| {
                serde_json::json!({
                    "name": span.name,
                    "cat": span.category,
                    "ph": "X",
                    "ts": span.start_us,
                    "dur": span.duration_us,
                    "pid": std::process::id(),
                    "tid": span.thread_id,
                })
            })
            .collect();
        serde_json::to_string_pretty(&serde_json::json!({
            "displayTimeUnit": "ms",
            "traceEvents": events,
        }))
        .unwrap_or_else(|_| "{\"traceEvents\":[]}".to_owned())
    }

    /// Converts retained spans into folded stack format (`category;name duration_us`).
    #[must_use]
    pub fn to_folded_stacks(&self) -> Vec<(String, u64)> {
        let mut counts: BTreeMap<String, u64> = BTreeMap::new();
        for span in &self.spans {
            let key = format!("{};{}", span.category, span.name);
            *counts.entry(key).or_insert(0) += span.duration_us.max(1);
        }
        counts.into_iter().collect()
    }
}

/// Parses macOS `/usr/bin/sample` indented call-graph lines into folded stacks `(stack, count)`.
#[must_use]
pub fn parse_macos_sample_to_folded(sample_output: &str) -> Vec<(String, u64)> {
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut in_call_graph = false;

    for raw_line in sample_output.lines() {
        if raw_line.contains("Call graph:") {
            in_call_graph = true;
            continue;
        }
        if !in_call_graph {
            continue;
        }
        if raw_line.trim().is_empty() || raw_line.starts_with("Total number in stack") {
            stack.clear();
            if raw_line.starts_with("Total number in stack") {
                in_call_graph = false;
            }
            continue;
        }

        let trimmed_start = raw_line.trim_start_matches([' ', '+', '|', '!', ':']);
        let prefix_len = raw_line.len().saturating_sub(trimmed_start.len());
        let mut parts = trimmed_start.split_whitespace();
        let Some(first) = parts.next() else {
            continue;
        };
        let Ok(samples) = first.parse::<u64>() else {
            continue;
        };
        let frame_raw = parts.collect::<Vec<_>>().join(" ");
        let frame_name = frame_raw
            .split("  (in ")
            .next()
            .unwrap_or(&frame_raw)
            .split(" + ")
            .next()
            .unwrap_or(&frame_raw)
            .trim()
            .to_owned();
        if frame_name.is_empty() {
            continue;
        }

        while let Some((depth, _)) = stack.last() {
            if *depth >= prefix_len {
                stack.pop();
            } else {
                break;
            }
        }
        stack.push((prefix_len, frame_name));
        let path = stack
            .iter()
            .map(|(_, frame)| frame.as_str())
            .collect::<Vec<_>>()
            .join(";");
        *counts.entry(path).or_insert(0) += samples;
    }

    counts.into_iter().collect()
}

/// Extracts the hottest leaf frames from folded stack counts in descending order.
#[must_use]
pub fn top_hot_frames(folded: &[(String, u64)], limit: usize) -> Vec<(String, u64)> {
    let mut by_leaf: BTreeMap<&str, u64> = BTreeMap::new();
    for (stack, weight) in folded {
        let leaf = stack.rsplit(';').next().unwrap_or(stack.as_str());
        *by_leaf.entry(leaf).or_insert(0) += *weight;
    }
    let mut items: Vec<(String, u64)> = by_leaf
        .into_iter()
        .map(|(name, weight)| (name.to_owned(), weight))
        .collect();
    items.sort_by_key(|item| std::cmp::Reverse(item.1));
    items.truncate(limit);
    items
}

/// Renders folded stack data (`stack;frame weight`) into a standalone interactive-tooltip SVG flamegraph.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    reason = "SVG pixel coordinates safely approximate total sample weights"
)]
pub fn render_svg_flamegraph(title: &str, folded: &[(String, u64)]) -> String {
    let width = 1200.0_f64;
    let frame_height = 20.0_f64;
    let margin_top = 44.0_f64;
    let margin_bottom = 28.0_f64;

    let total_weight: u64 = folded.iter().map(|(_, weight)| *weight).sum::<u64>().max(1);
    let max_depth = folded
        .iter()
        .map(|(stack, _)| stack.split(';').count())
        .max()
        .unwrap_or(1)
        .max(1);
    let height = margin_top + ((max_depth as f64) + 1.0) * frame_height + margin_bottom;

    // Group adjacent identical prefixes at each depth into merged horizontal bars.
    let mut bars: Vec<(usize, f64, f64, String, u64)> = Vec::new();
    let mut x_cursor = 0.0_f64;
    let mut sorted = folded.to_vec();
    sorted.sort_by(|left, right| left.0.cmp(&right.0));

    for (stack, weight) in &sorted {
        let w = ((*weight as f64) / (total_weight as f64)) * (width - 20.0);
        if w < 0.5 {
            x_cursor += w;
            continue;
        }
        for (depth, segment) in stack.split(';').enumerate() {
            if let Some(last) = bars.last_mut()
                && last.0 == depth
                && last.3 == segment
                && (last.1 + last.2 - (10.0 + x_cursor)).abs() < 0.01
            {
                last.2 += w;
                last.4 += *weight;
            } else {
                bars.push((depth, 10.0 + x_cursor, w, segment.to_owned(), *weight));
            }
        }
        x_cursor += w;
    }

    let mut svg = String::new();
    let _ = writeln!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {width} {height}\" width=\"{width}\" height=\"{height}\">"
    );
    svg.push_str("<rect width=\"100%\" height=\"100%\" fill=\"#0d1117\"/>\n");
    let _ = writeln!(
        svg,
        "<text x=\"20\" y=\"28\" fill=\"#f0f6fc\" font-family=\"ui-monospace,SFMono-Regular,Menlo,monospace\" font-size=\"16\" font-weight=\"bold\">{}</text>",
        escape_xml(title)
    );

    let baseline_y = height - margin_bottom - frame_height;
    let _ = writeln!(
        svg,
        "<g><title>all ({total_weight} units, 100.0%)</title><rect x=\"10\" y=\"{baseline_y}\" width=\"{}\" height=\"{}\" rx=\"2\" fill=\"#2f81f7\"/><text x=\"14\" y=\"{}\" fill=\"#ffffff\" font-family=\"ui-monospace,monospace\" font-size=\"11\">all ({total_weight})</text></g>",
        width - 20.0,
        frame_height - 1.0,
        baseline_y + 13.0
    );

    for (depth, x, bar_w, label, weight) in bars {
        let y = baseline_y - ((depth + 1) as f64) * frame_height;
        let pct = (weight as f64 / total_weight as f64) * 100.0;
        let color = flame_color(&label);
        let escaped = escape_xml(&label);
        let _ = write!(
            svg,
            "<g><title>{escaped} ({weight} units, {pct:.1}%)</title><rect x=\"{x:.2}\" y=\"{y:.2}\" width=\"{bar_w:.2}\" height=\"{}\" rx=\"2\" fill=\"{color}\" stroke=\"#0d1117\" stroke-width=\"0.6\"/>",
            frame_height - 1.0
        );
        if bar_w > 48.0 {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "bar_w > 48.0 guarantees a positive finite character count"
            )]
            let max_chars = ((bar_w - 8.0) / 6.8).floor().max(1.0) as usize;
            let short = if label.len() > max_chars {
                format!("{}…", &label[..max_chars.saturating_sub(1)])
            } else {
                label
            };
            let _ = write!(
                svg,
                "<text x=\"{:.2}\" y=\"{:.2}\" fill=\"#0d1117\" font-family=\"ui-monospace,monospace\" font-size=\"11\">{}</text>",
                x + 4.0,
                y + 13.0,
                escape_xml(&short)
            );
        }
        svg.push_str("</g>\n");
    }

    svg.push_str("</svg>\n");
    svg
}

fn flame_color(label: &str) -> &'static str {
    const PALETTE: [&str; 6] = [
        "#ff7b72", "#ffa657", "#f2cc60", "#ff9bce", "#d2a8ff", "#79c0ff",
    ];
    let hash = label.bytes().fold(0_usize, |acc, byte| {
        acc.wrapping_mul(31).wrapping_add(usize::from(byte))
    });
    PALETTE[hash % PALETTE.len()]
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Profiler, parse_macos_sample_to_folded, render_svg_flamegraph, top_hot_frames};

    #[test]
    fn profiler_summarizes_spans_frames_and_exports_chrome_trace() {
        let mut profiler = Profiler::new(64, 64, 60);
        profiler.measure("reduce", "state", || 42);
        profiler.measure("render", "ui", || 7);
        profiler.record_frame(Duration::from_millis(8));
        profiler.record_frame(Duration::from_millis(25));

        let summary = profiler.summarize();
        assert_eq!(summary.retained_span_count, 2);
        assert_eq!(summary.frames.total_frames, 2);
        assert_eq!(summary.frames.dropped_frames, 1);
        assert!(profiler.to_chrome_trace_json().contains("\"traceEvents\""));
        assert_eq!(profiler.to_folded_stacks().len(), 2);
    }

    #[test]
    fn parses_macos_sample_output_and_renders_svg_flamegraph() {
        let sample = "\
Analysis of sampling atlas (pid 1234) every 1 millisecond
Call graph:
    100 Thread_1000
      + 80 main  (in atlas) + 40
      +   50 App::reduce  (in atlas) + 12
      +   30 App::render  (in atlas) + 24
Total number in stack (recursive counted multiple, when >=5):
";
        let folded = parse_macos_sample_to_folded(sample);
        assert_ne!(folded.len(), 0);
        let hot = top_hot_frames(&folded, 3);
        assert_eq!(hot[0].0, "Thread_1000");

        let svg = render_svg_flamegraph("Atlas CPU Profile", &folded);
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("Atlas CPU Profile"));
        assert!(svg.contains("App::reduce"));
    }
}
