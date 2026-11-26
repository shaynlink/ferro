//! Cross-platform developer tools for Ferro: debugging, profiling, network inspection,
//! system/hardware telemetry, and loopback `DevTools` diagnostics.

pub mod debug;
pub mod network;
pub mod profiler;
pub mod server;
pub mod telemetry;

pub use debug::{
    BundleInspection, CrashReportEntry, DebugSession, EventTraceEntry, directory_size_bytes,
    inspect_macos_bundle, list_crash_reports,
};
pub use network::{
    NetworkFilter, NetworkHeader, NetworkInspector, NetworkProbeResult, NetworkSummary,
    NetworkTiming, NetworkTransaction, SocketConnection, inspect_sockets, probe_endpoint,
};
pub use profiler::{
    FrameSummary, ProfileSpan, ProfileSummary, Profiler, SpanStats, parse_macos_sample_to_folded,
    render_svg_flamegraph, top_hot_frames,
};
pub use server::{DevToolsDiscovery, DevToolsState, LocalDevToolsServer, WakeCallback};
pub use telemetry::{
    CpuTelemetry, DiskIoTelemetry, GpuTelemetry, MemoryTelemetry, NetworkInterfaceTelemetry,
    NetworkUsageTelemetry, NeuralEngineTelemetry, ProcessNetworkUsage, ProcessSample,
    ProcessTelemetry, SsdHealthTelemetry, StorageTelemetry, StorageVolumeTelemetry,
    SystemTelemetrySnapshot, TelemetryCollector,
};
