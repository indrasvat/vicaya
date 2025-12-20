//! `vicaya metrics`: on-demand runtime metrics for vicaya.

use clap::{Args, Subcommand};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use vicaya_core::ipc::{BuildInfo, Request, Response};
use vicaya_core::Result;

use crate::ipc_client::IpcClient;

const INNER_WIDTH: usize = 53;

#[derive(Args, Debug)]
pub(crate) struct MetricsArgs {
    /// Output format (pretty, json)
    #[arg(short, long, default_value = "pretty")]
    pub(crate) format: String,

    /// Skip `vmmap -summary` (macOS only); reduces overhead.
    #[arg(long)]
    pub(crate) no_vmmap: bool,

    #[command(subcommand)]
    pub(crate) action: Option<MetricsAction>,
}

#[derive(Subcommand, Debug)]
pub(crate) enum MetricsAction {
    /// Continuously print metrics.
    Watch(MetricsWatchArgs),
    /// Benchmark IPC search performance.
    Bench(MetricsBenchArgs),
}

#[derive(Args, Debug, Clone)]
pub(crate) struct MetricsWatchArgs {
    /// Output format (pretty, jsonl)
    #[arg(short, long, default_value = "pretty")]
    pub(crate) format: String,

    /// Sampling interval (e.g. 250ms, 1s, 2s)
    #[arg(long, default_value = "1s")]
    pub(crate) interval: String,

    /// Stop after N samples (default: run forever)
    #[arg(long)]
    pub(crate) count: Option<u64>,

    /// Capture `vmmap` every N ticks (default: 30). Set to 0 to disable.
    #[arg(long, default_value_t = 30)]
    pub(crate) vmmap_every: u64,
}

#[derive(Args, Debug, Clone)]
pub(crate) struct MetricsBenchArgs {
    /// Output format (pretty, json)
    #[arg(short, long, default_value = "pretty")]
    pub(crate) format: String,

    /// File containing queries to benchmark (one per line; `#` starts a comment).
    #[arg(long)]
    pub(crate) queries: PathBuf,

    /// Number of measured runs.
    #[arg(long, default_value_t = 200)]
    pub(crate) runs: u32,

    /// Number of warmup runs (not measured).
    #[arg(long, default_value_t = 25)]
    pub(crate) warmup: u32,

    /// Search result limit per query.
    #[arg(long, default_value_t = 20)]
    pub(crate) limit: usize,

    /// Capture `vmmap` before and after the benchmark.
    #[arg(long)]
    pub(crate) vmmap_before_after: bool,
}

pub(crate) fn run(args: MetricsArgs) -> Result<()> {
    match args.action {
        Some(MetricsAction::Watch(watch)) => watch_metrics(watch),
        Some(MetricsAction::Bench(bench)) => bench_metrics(bench),
        None => snapshot_metrics(&args.format, !args.no_vmmap),
    }
}

fn snapshot_metrics(format: &str, include_vmmap: bool) -> Result<()> {
    let ctx = build_snapshot_context()?;
    let snapshot = collect_snapshot(SnapshotMode::OneShot { include_vmmap }, &ctx)?;
    match format {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&snapshot).unwrap());
        }
        _ => {
            print_pretty_snapshot(&snapshot, true);
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum SnapshotMode {
    OneShot { include_vmmap: bool },
    WatchTick { include_vmmap: bool, tick: u64 },
}

#[derive(Debug, Serialize)]
struct MetricsSnapshot {
    schema_version: u32,
    tick: Option<u64>,
    captured_at_unix_ms: i64,
    captured_at_rfc3339: String,
    client: ClientSnapshot,
    daemon: DaemonSnapshot,
    index: Option<IndexSnapshot>,
    disk: DiskSnapshot,
    process: Option<ProcessSnapshot>,
    derived: DerivedSnapshot,
    notes: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
struct ClientSnapshot {
    build: BuildInfo,
    cwd: Option<String>,
    args: Vec<String>,
}

#[derive(Debug, Serialize)]
struct DaemonSnapshot {
    running: bool,
    pid: Option<i32>,
    pid_file: String,
    socket_path: String,
    build: Option<BuildInfo>,
    connect_error: Option<String>,
}

#[derive(Debug, Serialize)]
struct IndexSnapshot {
    files: usize,
    trigrams: usize,
    arena_bytes: usize,
    index_allocated_bytes: u64,
    state_allocated_bytes: u64,
    last_updated: i64,
    reconciling: bool,
}

#[derive(Debug, Serialize)]
struct DiskSnapshot {
    config_path: String,
    index_dir: String,
    index_file: FileSnapshot,
    journal_file: FileSnapshot,
}

#[derive(Debug, Serialize)]
struct FileSnapshot {
    path: String,
    exists: bool,
    size_bytes: u64,
    modified_unix_ms: Option<i64>,
}

#[derive(Debug, Serialize)]
struct ProcessSnapshot {
    pid: i32,
    ps: PsSnapshot,
    vmmap: VmmapSnapshot,
}

#[derive(Debug, Serialize)]
struct PsSnapshot {
    captured: bool,
    command: String,
    ok: bool,
    error: Option<String>,
    rss_bytes: Option<u64>,
    etime: Option<String>,
}

#[derive(Debug, Serialize)]
struct VmmapSnapshot {
    captured: bool,
    command: String,
    ok: bool,
    error: Option<String>,
    physical_footprint_bytes: Option<u64>,
    physical_footprint_peak_bytes: Option<u64>,
    total: Option<VmmapTotals>,
    malloc_zone_total: Option<VmmapMallocTotals>,
}

#[derive(Debug, Serialize)]
struct VmmapTotals {
    virtual_bytes: u64,
    resident_bytes: u64,
    dirty_bytes: u64,
    swapped_bytes: u64,
    volatile_bytes: u64,
    nonvol_bytes: u64,
    empty_bytes: u64,
    region_count: u64,
}

#[derive(Debug, Serialize)]
struct VmmapMallocTotals {
    virtual_bytes: u64,
    resident_bytes: u64,
    dirty_bytes: u64,
    swapped_bytes: u64,
    allocation_count: u64,
    allocated_bytes: u64,
    frag_bytes: u64,
    frag_pct: f64,
    region_count: u64,
}

#[derive(Debug, Default, Serialize)]
struct DerivedSnapshot {
    bytes_per_file_arena: Option<u64>,
    bytes_per_file_state_heap_est: Option<u64>,
    bytes_per_file_physical_footprint: Option<u64>,
    heap_est_to_footprint_ratio: Option<f64>,
}

#[derive(Debug, Clone)]
struct SnapshotContext {
    client: ClientSnapshot,
    config_path: String,
    index_dir: String,
    index_file_path: PathBuf,
    journal_file_path: PathBuf,
    pid_file: String,
    socket_path: String,
}

fn build_snapshot_context() -> Result<SnapshotContext> {
    let client = ClientSnapshot {
        build: client_build_info(),
        cwd: std::env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().to_string()),
        args: std::env::args().collect(),
    };

    let config = crate::load_config()?;
    let config_path = vicaya_core::paths::config_path();
    let index_dir = config.index_path;

    Ok(SnapshotContext {
        client,
        config_path: config_path.to_string_lossy().to_string(),
        index_dir: index_dir.to_string_lossy().to_string(),
        index_file_path: index_dir.join("index.bin"),
        journal_file_path: index_dir.join("index.journal"),
        pid_file: vicaya_core::daemon::pid_file_path()
            .to_string_lossy()
            .to_string(),
        socket_path: vicaya_core::ipc::socket_path()
            .to_string_lossy()
            .to_string(),
    })
}

fn collect_snapshot(mode: SnapshotMode, ctx: &SnapshotContext) -> Result<MetricsSnapshot> {
    let now = chrono::Utc::now();
    let captured_at_unix_ms = now.timestamp_millis();
    let captured_at_rfc3339 = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    let disk = DiskSnapshot {
        config_path: ctx.config_path.clone(),
        index_dir: ctx.index_dir.clone(),
        index_file: stat_file(&ctx.index_file_path),
        journal_file: stat_file(&ctx.journal_file_path),
    };

    let mut notes = Vec::new();

    let running = vicaya_core::daemon::is_running();
    let pid = vicaya_core::daemon::get_pid();

    let mut daemon_build = None;
    let mut connect_error = None;
    let mut index = None;

    if running {
        match IpcClient::connect() {
            Ok(mut client) => match client.request(&Request::Status) {
                Ok(Response::Status {
                    pid: status_pid,
                    build,
                    indexed_files,
                    trigram_count,
                    arena_size,
                    index_allocated_bytes,
                    state_allocated_bytes,
                    last_updated,
                    reconciling,
                }) => {
                    daemon_build = Some(build);
                    if pid.is_none() && status_pid > 0 {
                        // PID file may be missing; prefer daemon-reported PID when available.
                        notes.push("PID file missing; using daemon-reported PID".to_string());
                    }
                    index = Some(IndexSnapshot {
                        files: indexed_files,
                        trigrams: trigram_count,
                        arena_bytes: arena_size,
                        index_allocated_bytes,
                        state_allocated_bytes,
                        last_updated,
                        reconciling,
                    });
                }
                Ok(Response::Error { message }) => {
                    connect_error = Some(message);
                }
                Ok(_) => {
                    connect_error = Some("Unexpected response from daemon".to_string());
                }
                Err(e) => {
                    connect_error = Some(e.to_string());
                }
            },
            Err(e) => {
                connect_error = Some(e.to_string());
            }
        }
    } else {
        notes.push("Daemon is not running".to_string());
    }

    let daemon = DaemonSnapshot {
        running,
        pid,
        pid_file: ctx.pid_file.clone(),
        socket_path: ctx.socket_path.clone(),
        build: daemon_build,
        connect_error,
    };

    let (include_vmmap, tick) = match mode {
        SnapshotMode::OneShot { include_vmmap } => (include_vmmap, None),
        SnapshotMode::WatchTick {
            include_vmmap,
            tick,
        } => (include_vmmap, Some(tick)),
    };

    let process = pid.map(|pid| collect_process(pid, include_vmmap));

    let derived = derive_metrics(index.as_ref(), process.as_ref());

    Ok(MetricsSnapshot {
        schema_version: 1,
        tick,
        captured_at_unix_ms,
        captured_at_rfc3339,
        client: ctx.client.clone(),
        daemon,
        index,
        disk,
        process,
        derived,
        notes,
    })
}

fn client_build_info() -> BuildInfo {
    let b = vicaya_core::build_info::BUILD_INFO;
    BuildInfo {
        version: b.version.to_string(),
        git_sha: b.git_sha.to_string(),
        timestamp: b.timestamp.to_string(),
        target: b.target.to_string(),
    }
}

fn stat_file(path: &Path) -> FileSnapshot {
    match std::fs::metadata(path) {
        Ok(meta) => FileSnapshot {
            path: path.to_string_lossy().to_string(),
            exists: true,
            size_bytes: meta.len(),
            modified_unix_ms: meta.modified().ok().and_then(system_time_to_unix_ms),
        },
        Err(_) => FileSnapshot {
            path: path.to_string_lossy().to_string(),
            exists: false,
            size_bytes: 0,
            modified_unix_ms: None,
        },
    }
}

fn system_time_to_unix_ms(t: SystemTime) -> Option<i64> {
    let d = t.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(d.as_millis()).ok()
}

fn collect_process(pid: i32, include_vmmap: bool) -> ProcessSnapshot {
    let ps = collect_ps(pid);

    let vmmap = if include_vmmap {
        collect_vmmap(pid)
    } else {
        VmmapSnapshot {
            captured: false,
            command: format!("vmmap -summary {pid}"),
            ok: false,
            error: None,
            physical_footprint_bytes: None,
            physical_footprint_peak_bytes: None,
            total: None,
            malloc_zone_total: None,
        }
    };

    ProcessSnapshot { pid, ps, vmmap }
}

fn collect_ps(pid: i32) -> PsSnapshot {
    let command = format!("ps -p {pid} -o rss= -o etime=");
    let output = Command::new("ps")
        .arg("-p")
        .arg(pid.to_string())
        .arg("-o")
        .arg("rss=")
        .arg("-o")
        .arg("etime=")
        .output();

    let Ok(output) = output else {
        return PsSnapshot {
            captured: true,
            command,
            ok: false,
            error: Some("Failed to spawn ps".to_string()),
            rss_bytes: None,
            etime: None,
        };
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return PsSnapshot {
            captured: true,
            command,
            ok: false,
            error: Some(if stderr.is_empty() {
                format!("ps exited with status {}", output.status)
            } else {
                stderr
            }),
            rss_bytes: None,
            etime: None,
        };
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let tokens: Vec<&str> = stdout.split_whitespace().collect();
    let rss_kib = tokens.first().and_then(|t| t.parse::<u64>().ok());
    let etime = tokens.get(1).map(|s| s.to_string());

    PsSnapshot {
        captured: true,
        command,
        ok: rss_kib.is_some(),
        error: if rss_kib.is_some() {
            None
        } else {
            Some(format!("Unexpected ps output: {}", stdout.trim()))
        },
        rss_bytes: rss_kib.map(|k| k.saturating_mul(1024)),
        etime,
    }
}

fn collect_vmmap(pid: i32) -> VmmapSnapshot {
    let command = format!("vmmap -summary {pid}");
    let output = Command::new("vmmap")
        .arg("-summary")
        .arg(pid.to_string())
        .output();

    let Ok(output) = output else {
        return VmmapSnapshot {
            captured: true,
            command,
            ok: false,
            error: Some("Failed to spawn vmmap".to_string()),
            physical_footprint_bytes: None,
            physical_footprint_peak_bytes: None,
            total: None,
            malloc_zone_total: None,
        };
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return VmmapSnapshot {
            captured: true,
            command,
            ok: false,
            error: Some(if stderr.is_empty() {
                format!("vmmap exited with status {}", output.status)
            } else {
                stderr
            }),
            physical_footprint_bytes: None,
            physical_footprint_peak_bytes: None,
            total: None,
            malloc_zone_total: None,
        };
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed = parse_vmmap_summary(&stdout);

    VmmapSnapshot {
        captured: true,
        command,
        ok: parsed.ok,
        error: parsed.error,
        physical_footprint_bytes: parsed.physical_footprint_bytes,
        physical_footprint_peak_bytes: parsed.physical_footprint_peak_bytes,
        total: parsed.total,
        malloc_zone_total: parsed.malloc_zone_total,
    }
}

#[derive(Debug, Default)]
struct VmmapParsed {
    ok: bool,
    error: Option<String>,
    physical_footprint_bytes: Option<u64>,
    physical_footprint_peak_bytes: Option<u64>,
    total: Option<VmmapTotals>,
    malloc_zone_total: Option<VmmapMallocTotals>,
}

fn parse_vmmap_summary(output: &str) -> VmmapParsed {
    let mut parsed = VmmapParsed::default();

    let mut in_malloc_zone = false;
    for line in output.lines() {
        let t = line.trim();

        if let Some(v) = t.strip_prefix("Physical footprint:") {
            parsed.physical_footprint_bytes = parse_vmmap_size_to_bytes(v.trim());
            continue;
        }
        if let Some(v) = t.strip_prefix("Physical footprint (peak):") {
            parsed.physical_footprint_peak_bytes = parse_vmmap_size_to_bytes(v.trim());
            continue;
        }

        if t.starts_with("MALLOC ZONE") {
            in_malloc_zone = true;
            continue;
        }

        if t.starts_with("TOTAL") {
            if in_malloc_zone {
                if parsed.malloc_zone_total.is_none() {
                    parsed.malloc_zone_total = parse_vmmap_malloc_total_line(t);
                }
            } else if parsed.total.is_none() {
                parsed.total = parse_vmmap_total_line(t);
            }
        }
    }

    parsed.ok = parsed.physical_footprint_bytes.is_some()
        || parsed.physical_footprint_peak_bytes.is_some()
        || parsed.total.is_some()
        || parsed.malloc_zone_total.is_some();

    if !parsed.ok {
        parsed.error = Some("Failed to parse vmmap summary".to_string());
    }

    parsed
}

fn parse_vmmap_total_line(line: &str) -> Option<VmmapTotals> {
    // Example:
    // TOTAL  1.9G  233.4M  4113K  0K  0K  0K  0K  1529
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.len() < 9 {
        return None;
    }
    Some(VmmapTotals {
        virtual_bytes: parse_vmmap_size_to_bytes(tokens[1])?,
        resident_bytes: parse_vmmap_size_to_bytes(tokens[2])?,
        dirty_bytes: parse_vmmap_size_to_bytes(tokens[3])?,
        swapped_bytes: parse_vmmap_size_to_bytes(tokens[4])?,
        volatile_bytes: parse_vmmap_size_to_bytes(tokens[5])?,
        nonvol_bytes: parse_vmmap_size_to_bytes(tokens[6])?,
        empty_bytes: parse_vmmap_size_to_bytes(tokens[7])?,
        region_count: tokens[8].parse::<u64>().ok()?,
    })
}

fn parse_vmmap_malloc_total_line(line: &str) -> Option<VmmapMallocTotals> {
    // Example:
    // TOTAL  950.0M  2464K  2464K  0K  10156  493K  1971K  81%  16
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.len() < 10 {
        return None;
    }
    let frag_pct = tokens[8].trim_end_matches('%').parse::<f64>().ok()?;
    Some(VmmapMallocTotals {
        virtual_bytes: parse_vmmap_size_to_bytes(tokens[1])?,
        resident_bytes: parse_vmmap_size_to_bytes(tokens[2])?,
        dirty_bytes: parse_vmmap_size_to_bytes(tokens[3])?,
        swapped_bytes: parse_vmmap_size_to_bytes(tokens[4])?,
        allocation_count: tokens[5].parse::<u64>().ok()?,
        allocated_bytes: parse_vmmap_size_to_bytes(tokens[6])?,
        frag_bytes: parse_vmmap_size_to_bytes(tokens[7])?,
        frag_pct,
        region_count: tokens[9].parse::<u64>().ok()?,
    })
}

fn parse_vmmap_size_to_bytes(token: &str) -> Option<u64> {
    let s = token.trim();
    if s.is_empty() {
        return None;
    }

    let (num_str, unit) = match s.chars().last()? {
        c if c.is_ascii_alphabetic() => (&s[..s.len().saturating_sub(1)], Some(c)),
        _ => (s, None),
    };

    let value = num_str.trim().parse::<f64>().ok()?;

    let mul = match unit.map(|c| c.to_ascii_uppercase()) {
        None => 1.0,
        Some('K') => 1024.0,
        Some('M') => 1024.0 * 1024.0,
        Some('G') => 1024.0 * 1024.0 * 1024.0,
        Some('T') => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        Some(_) => return None,
    };

    let bytes = value * mul;
    if !bytes.is_finite() || bytes < 0.0 {
        return None;
    }
    Some(bytes.round() as u64)
}

fn derive_metrics(
    index: Option<&IndexSnapshot>,
    process: Option<&ProcessSnapshot>,
) -> DerivedSnapshot {
    let mut derived = DerivedSnapshot::default();

    if let Some(index) = index {
        if index.files > 0 {
            derived.bytes_per_file_arena = Some((index.arena_bytes as u64) / index.files as u64);
            derived.bytes_per_file_state_heap_est =
                Some(index.state_allocated_bytes / index.files as u64);
        }
    }

    if let (Some(index), Some(process)) = (index, process) {
        if index.files > 0 {
            if let Some(footprint) = process.vmmap.physical_footprint_bytes {
                derived.bytes_per_file_physical_footprint = Some(footprint / index.files as u64);
                if index.state_allocated_bytes > 0 {
                    derived.heap_est_to_footprint_ratio =
                        Some(footprint as f64 / index.state_allocated_bytes as f64);
                }
            }
        }
    }

    derived
}

fn print_pretty_snapshot(snapshot: &MetricsSnapshot, padded: bool) {
    use owo_colors::OwoColorize;

    if padded {
        println!();
    }
    println!(
        "{}",
        "╭───────────────────────────────────────────────────────╮".bright_blue()
    );
    println!(
        "{} {:<53} {}",
        "│".bright_blue(),
        "Vicaya — Runtime Metrics".bold().bright_white(),
        "│".bright_blue()
    );

    println!(
        "{}",
        "├───────────────────────────────────────────────────────┤".bright_blue()
    );

    let title_line = format!("{:53}", "  Daemon");
    println!(
        "{} {} {}",
        "│".bright_blue(),
        title_line.bold().bright_white(),
        "│".bright_blue()
    );

    let running_str = if snapshot.daemon.running {
        "● Running"
    } else {
        "○ Not running"
    };
    print_kv_line(
        "    Status:",
        running_str,
        if snapshot.daemon.running {
            ValueStyle::Good
        } else {
            ValueStyle::Warn
        },
    );

    if let Some(pid) = snapshot.daemon.pid {
        print_kv_line("    PID:", &pid.to_string(), ValueStyle::Neutral);
    }

    if let Some(build) = snapshot.daemon.build.as_ref() {
        let build_str = format!(
            "{} (rev {})",
            build.version,
            if build.git_sha.is_empty() {
                "unknown"
            } else {
                &build.git_sha
            }
        );
        print_kv_line("    Build:", &build_str, ValueStyle::Neutral);
    }

    if let Some(err) = snapshot.daemon.connect_error.as_ref() {
        print_kv_line("    IPC:", err, ValueStyle::Warn);
    }

    println!(
        "{}",
        "├───────────────────────────────────────────────────────┤".bright_blue()
    );

    let title_line = format!("{:53}", "  Process Memory (macOS)");
    println!(
        "{} {} {}",
        "│".bright_blue(),
        title_line.bold().bright_white(),
        "│".bright_blue()
    );

    if let Some(process) = snapshot.process.as_ref() {
        if let Some(rss) = process.ps.rss_bytes {
            print_kv_line("    RSS (ps):", &format_bytes_mb(rss), ValueStyle::Neutral);
        }

        if process.vmmap.captured {
            if let Some(foot) = process.vmmap.physical_footprint_bytes {
                print_kv_line(
                    "    Physical footprint:",
                    &format_bytes_mb(foot),
                    ValueStyle::Hot,
                );
            }
            if let Some(peak) = process.vmmap.physical_footprint_peak_bytes {
                print_kv_line(
                    "    Footprint (peak):",
                    &format_bytes_mb(peak),
                    ValueStyle::Neutral,
                );
            }
            if let Some(malloc) = process.vmmap.malloc_zone_total.as_ref() {
                print_kv_line(
                    "    Malloc allocated:",
                    &format_bytes_mb(malloc.allocated_bytes),
                    ValueStyle::Hot,
                );
                print_kv_line(
                    "    Malloc frag:",
                    &format!(
                        "{} ({}%)",
                        format_bytes_mb(malloc.frag_bytes),
                        malloc.frag_pct
                    ),
                    ValueStyle::Neutral,
                );
            }
            if let Some(total) = process.vmmap.total.as_ref() {
                print_kv_line(
                    "    Swapped:",
                    &format_bytes_mb(total.swapped_bytes),
                    ValueStyle::Neutral,
                );
            }
            if let Some(err) = process.vmmap.error.as_ref() {
                print_kv_line("    vmmap:", err, ValueStyle::Warn);
            }
        } else {
            print_kv_line("    vmmap:", "skipped", ValueStyle::Neutral);
        }
    } else {
        print_kv_line("    PID:", "unavailable", ValueStyle::Warn);
    }

    println!(
        "{}",
        "├───────────────────────────────────────────────────────┤".bright_blue()
    );

    let title_line = format!("{:53}", "  Index");
    println!(
        "{} {} {}",
        "│".bright_blue(),
        title_line.bold().bright_white(),
        "│".bright_blue()
    );

    if let Some(index) = snapshot.index.as_ref() {
        print_kv_line(
            "    Files indexed:",
            &crate::format_number(index.files),
            ValueStyle::Good,
        );
        print_kv_line(
            "    Trigrams:",
            &crate::format_number(index.trigrams),
            ValueStyle::Neutral,
        );
        print_kv_line(
            "    State heap (est):",
            &format_bytes_mb(index.state_allocated_bytes),
            ValueStyle::Hot,
        );
        print_kv_line(
            "    Index heap (est):",
            &format_bytes_mb(index.index_allocated_bytes),
            ValueStyle::Neutral,
        );
        print_kv_line(
            "    String arena:",
            &format_bytes_mb(index.arena_bytes as u64),
            ValueStyle::Neutral,
        );
        if index.reconciling {
            print_kv_line("    Reconcile:", "running", ValueStyle::Warn);
        }
    } else {
        print_kv_line("    Status:", "unavailable", ValueStyle::Warn);
    }

    println!(
        "{}",
        "├───────────────────────────────────────────────────────┤".bright_blue()
    );

    let title_line = format!("{:53}", "  Disk");
    println!(
        "{} {} {}",
        "│".bright_blue(),
        title_line.bold().bright_white(),
        "│".bright_blue()
    );

    let index_size = snapshot.disk.index_file.size_bytes;
    let journal_size = snapshot.disk.journal_file.size_bytes;
    print_kv_line(
        "    index.bin:",
        &format_bytes_mb(index_size),
        ValueStyle::Neutral,
    );
    print_kv_line(
        "    index.journal:",
        &format_bytes_mb(journal_size),
        ValueStyle::Neutral,
    );

    println!(
        "{}",
        "├───────────────────────────────────────────────────────┤".bright_blue()
    );

    let title_line = format!("{:53}", "  Derived");
    println!(
        "{} {} {}",
        "│".bright_blue(),
        title_line.bold().bright_white(),
        "│".bright_blue()
    );

    if let Some(bpf) = snapshot.derived.bytes_per_file_arena {
        print_kv_line(
            "    Arena/file:",
            &format!("{} B", bpf),
            ValueStyle::Neutral,
        );
    }
    if let Some(bpf) = snapshot.derived.bytes_per_file_state_heap_est {
        print_kv_line(
            "    Heap est/file:",
            &format!("{} B", bpf),
            ValueStyle::Neutral,
        );
    }
    if let Some(bpf) = snapshot.derived.bytes_per_file_physical_footprint {
        print_kv_line(
            "    Footprint/file:",
            &format!("{} B", bpf),
            ValueStyle::Hot,
        );
    }
    if let Some(ratio) = snapshot.derived.heap_est_to_footprint_ratio {
        print_kv_line(
            "    Footprint/heap:",
            &format!("{:.2}×", ratio),
            ValueStyle::Neutral,
        );
    }

    println!(
        "{}",
        "╰───────────────────────────────────────────────────────╯".bright_blue()
    );
    if padded {
        println!();
    }
}

#[derive(Clone, Copy)]
enum ValueStyle {
    Good,
    Warn,
    Hot,
    Neutral,
}

fn print_kv_line(label: &str, value: &str, style: ValueStyle) {
    use owo_colors::OwoColorize;

    let available = INNER_WIDTH.saturating_sub(label.len());
    let value_aligned = fit_value_right(value, available);

    let border = "│".bright_blue();
    let label = label.dimmed();
    match style {
        ValueStyle::Good => println!(
            "{border} {label}{} {border}",
            value_aligned.bright_green().bold()
        ),
        ValueStyle::Warn => println!(
            "{border} {label}{} {border}",
            value_aligned.bright_yellow().bold()
        ),
        ValueStyle::Hot => println!(
            "{border} {label}{} {border}",
            value_aligned.bright_magenta()
        ),
        ValueStyle::Neutral => {
            println!("{border} {label}{} {border}", value_aligned.bright_white())
        }
    };
}

fn fit_value_right(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let value = value.trim();
    let len = value.chars().count();

    if len <= width {
        return format!("{value:>width$}");
    }

    if width <= 3 {
        return ".".repeat(width);
    }

    let tail_len = width - 3;
    let mut tail: String = value.chars().rev().take(tail_len).collect();
    tail = tail.chars().rev().collect();
    format!("...{tail}")
}

fn format_bytes_mb(bytes: u64) -> String {
    let mb = bytes as f64 / 1_048_576.0;
    if mb >= 1024.0 {
        format!("{:.2} GB", mb / 1024.0)
    } else {
        format!("{:.1} MB", mb)
    }
}

fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let (num, unit) = if s.ends_with("ms") {
        (&s[..s.len().saturating_sub(2)], "ms")
    } else if s.ends_with('s') {
        (&s[..s.len().saturating_sub(1)], "s")
    } else if s.ends_with('m') {
        (&s[..s.len().saturating_sub(1)], "m")
    } else {
        (s, "s")
    };

    let value: u64 = num.trim().parse().ok()?;

    Some(match unit {
        "ms" => Duration::from_millis(value),
        "s" => Duration::from_secs(value),
        "m" => Duration::from_secs(value.saturating_mul(60)),
        _ => return None,
    })
}

fn watch_metrics(args: MetricsWatchArgs) -> Result<()> {
    use std::io::IsTerminal;
    use std::io::Write;

    let interval = parse_duration(&args.interval)
        .ok_or_else(|| vicaya_core::Error::Config("Invalid --interval".to_string()))?;

    let is_tty = std::io::stdout().is_terminal();
    let mut tick: u64 = 0;
    let ctx = build_snapshot_context()?;

    loop {
        let include_vmmap = args.vmmap_every > 0 && tick.is_multiple_of(args.vmmap_every);
        let snapshot = collect_snapshot(
            SnapshotMode::WatchTick {
                include_vmmap,
                tick,
            },
            &ctx,
        )?;

        match args.format.as_str() {
            "jsonl" => {
                println!("{}", serde_json::to_string(&snapshot).unwrap());
            }
            _ => {
                if is_tty {
                    // Clear + home cursor.
                    print!("\x1B[2J\x1B[H");
                    std::io::stdout().flush().ok();
                }
                print_pretty_snapshot(&snapshot, !is_tty);
            }
        }

        tick += 1;

        if let Some(count) = args.count {
            if tick >= count {
                break;
            }
        }

        std::thread::sleep(interval);
    }

    Ok(())
}

#[derive(Debug, Serialize)]
struct BenchReport {
    schema_version: u32,
    captured_at_unix_ms: i64,
    captured_at_rfc3339: String,
    client: ClientSnapshot,
    daemon: DaemonSnapshot,
    index: Option<IndexSnapshot>,
    params: BenchParams,
    vmmap_before: Option<VmmapSnapshot>,
    vmmap_after: Option<VmmapSnapshot>,
    summary: BenchSummary,
    samples: Vec<u64>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct BenchParams {
    runs: u32,
    warmup: u32,
    limit: usize,
    query_count: usize,
}

#[derive(Debug, Serialize)]
struct BenchSummary {
    ok_runs: u32,
    error_runs: u32,
    min_us: u64,
    p50_us: u64,
    p90_us: u64,
    p95_us: u64,
    p99_us: u64,
    max_us: u64,
    mean_us: u64,
    total_time_ms: u64,
    qps: f64,
}

fn bench_metrics(args: MetricsBenchArgs) -> Result<()> {
    use owo_colors::OwoColorize;
    use std::time::Instant;

    let queries = load_queries(&args.queries)?;
    if queries.is_empty() {
        return Err(vicaya_core::Error::Config(
            "No queries found in --queries file".to_string(),
        ));
    }

    let now = chrono::Utc::now();
    let captured_at_unix_ms = now.timestamp_millis();
    let captured_at_rfc3339 = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    let client = ClientSnapshot {
        build: client_build_info(),
        cwd: std::env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().to_string()),
        args: std::env::args().collect(),
    };

    let running = vicaya_core::daemon::is_running();
    if !running {
        return Err(vicaya_core::Error::Config(
            "Daemon is not running; start it before benchmarking".to_string(),
        ));
    }

    let pid = vicaya_core::daemon::get_pid();
    let mut daemon_build = None;
    let mut connect_error = None;
    let mut index = None;

    // Capture daemon status once for the report.
    if let Ok(mut client_ipc) = IpcClient::connect() {
        match client_ipc.request(&Request::Status) {
            Ok(Response::Status {
                build,
                indexed_files,
                trigram_count,
                arena_size,
                index_allocated_bytes,
                state_allocated_bytes,
                last_updated,
                reconciling,
                ..
            }) => {
                daemon_build = Some(build);
                index = Some(IndexSnapshot {
                    files: indexed_files,
                    trigrams: trigram_count,
                    arena_bytes: arena_size,
                    index_allocated_bytes,
                    state_allocated_bytes,
                    last_updated,
                    reconciling,
                });
            }
            Ok(Response::Error { message }) => connect_error = Some(message),
            Ok(_) => connect_error = Some("Unexpected response from daemon".to_string()),
            Err(e) => connect_error = Some(e.to_string()),
        }
    } else {
        connect_error = Some("Failed to connect to daemon".to_string());
    }

    let daemon = DaemonSnapshot {
        running,
        pid,
        pid_file: vicaya_core::daemon::pid_file_path()
            .to_string_lossy()
            .to_string(),
        socket_path: vicaya_core::ipc::socket_path()
            .to_string_lossy()
            .to_string(),
        build: daemon_build,
        connect_error,
    };

    let mut notes = Vec::new();
    let mut vmmap_before = None;
    let mut vmmap_after = None;

    if args.vmmap_before_after {
        if let Some(pid) = pid {
            vmmap_before = Some(collect_vmmap(pid));
        } else {
            notes.push("PID unavailable; skipping vmmap before/after".to_string());
        }
    }

    // Warmup.
    for i in 0..args.warmup {
        let q = &queries[i as usize % queries.len()];
        let request = Request::Search {
            query: q.clone(),
            limit: args.limit,
            scope: std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().to_string()),
        };
        if let Ok(mut client_ipc) = IpcClient::connect() {
            let _ = client_ipc.request(&request);
        }
    }

    let mut samples_us: Vec<u64> = Vec::with_capacity(args.runs as usize);
    let mut error_runs: u32 = 0;

    let start_all = Instant::now();
    for i in 0..args.runs {
        let q = &queries[i as usize % queries.len()];
        let request = Request::Search {
            query: q.clone(),
            limit: args.limit,
            scope: std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().to_string()),
        };

        let start = Instant::now();
        let resp = match IpcClient::connect() {
            Ok(mut client_ipc) => client_ipc.request(&request),
            Err(e) => Err(e),
        };
        let elapsed = start.elapsed();

        match resp {
            Ok(Response::SearchResults { .. }) => {
                samples_us.push(elapsed.as_micros().min(u128::from(u64::MAX)) as u64);
            }
            Ok(Response::Error { message }) => {
                error_runs += 1;
                notes.push(format!("daemon error: {message}"));
            }
            Ok(_) => {
                error_runs += 1;
                notes.push("unexpected response during bench".to_string());
            }
            Err(e) => {
                error_runs += 1;
                notes.push(format!("ipc error: {e}"));
            }
        }
    }
    let total_time = start_all.elapsed();

    if args.vmmap_before_after {
        if let Some(pid) = pid {
            vmmap_after = Some(collect_vmmap(pid));
        }
    }

    let mut sorted = samples_us.clone();
    sorted.sort_unstable();
    let ok_runs = sorted.len() as u32;

    let summary = summarize_latencies(&sorted, ok_runs, error_runs, total_time);

    let report = BenchReport {
        schema_version: 1,
        captured_at_unix_ms,
        captured_at_rfc3339,
        client,
        daemon,
        index,
        params: BenchParams {
            runs: args.runs,
            warmup: args.warmup,
            limit: args.limit,
            query_count: queries.len(),
        },
        vmmap_before,
        vmmap_after,
        summary,
        samples: samples_us,
        notes,
    };

    match args.format.as_str() {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        }
        _ => {
            println!();
            println!("{}", "Vicaya — Bench".bold().bright_white());
            println!(
                "  Runs: {} (warmup {}) | Queries: {} | Limit: {}",
                args.runs,
                args.warmup,
                queries.len(),
                args.limit
            );
            if let Some(index) = report.index.as_ref() {
                println!(
                    "  Index: {} files | est heap {}",
                    crate::format_number(index.files),
                    format_bytes_mb(index.state_allocated_bytes)
                );
            }
            println!();
            println!(
                "  min {:>8}  p50 {:>8}  p90 {:>8}  p95 {:>8}  p99 {:>8}  max {:>8}",
                format_us(report.summary.min_us),
                format_us(report.summary.p50_us),
                format_us(report.summary.p90_us),
                format_us(report.summary.p95_us),
                format_us(report.summary.p99_us),
                format_us(report.summary.max_us),
            );
            println!(
                "  mean {:>7}  total {:>7}  qps {:>7.1}  errors {}",
                format_us(report.summary.mean_us),
                format!("{}ms", report.summary.total_time_ms),
                report.summary.qps,
                report.summary.error_runs
            );
            if let (Some(before), Some(after)) =
                (report.vmmap_before.as_ref(), report.vmmap_after.as_ref())
            {
                if let (Some(b), Some(a)) = (
                    before.physical_footprint_bytes,
                    after.physical_footprint_bytes,
                ) {
                    let delta = a.saturating_sub(b);
                    println!(
                        "  Footprint: {} → {} (Δ {})",
                        format_bytes_mb(b),
                        format_bytes_mb(a),
                        format_bytes_mb(delta)
                    );
                }
            }
            println!();
        }
    }

    Ok(())
}

fn load_queries(path: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(path)?;
    let mut out = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        out.push(trimmed.to_string());
    }
    Ok(out)
}

fn summarize_latencies(
    samples_sorted_us: &[u64],
    ok_runs: u32,
    error_runs: u32,
    total_time: Duration,
) -> BenchSummary {
    let n = samples_sorted_us.len();
    let min_us = samples_sorted_us.first().copied().unwrap_or(0);
    let max_us = samples_sorted_us.last().copied().unwrap_or(0);

    let p50_us = percentile(samples_sorted_us, 50.0);
    let p90_us = percentile(samples_sorted_us, 90.0);
    let p95_us = percentile(samples_sorted_us, 95.0);
    let p99_us = percentile(samples_sorted_us, 99.0);

    let mean_us = if n == 0 {
        0
    } else {
        let sum: u128 = samples_sorted_us.iter().map(|&v| v as u128).sum();
        (sum / n as u128).min(u128::from(u64::MAX)) as u64
    };

    let total_time_ms = total_time.as_millis().min(u128::from(u64::MAX)) as u64;
    let qps = if total_time.as_secs_f64() > 0.0 {
        ok_runs as f64 / total_time.as_secs_f64()
    } else {
        0.0
    };

    BenchSummary {
        ok_runs,
        error_runs,
        min_us,
        p50_us,
        p90_us,
        p95_us,
        p99_us,
        max_us,
        mean_us,
        total_time_ms,
        qps,
    }
}

fn percentile(sorted: &[u64], pct: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let clamped = pct.clamp(0.0, 100.0);
    let rank = (clamped / 100.0) * (sorted.len() - 1) as f64;
    sorted[rank.round() as usize]
}

fn format_us(us: u64) -> String {
    if us >= 1_000_000 {
        format!("{:.1}s", us as f64 / 1_000_000.0)
    } else if us >= 1_000 {
        format!("{:.1}ms", us as f64 / 1_000.0)
    } else {
        format!("{us}µs")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vmmap_sizes() {
        assert_eq!(parse_vmmap_size_to_bytes("0K"), Some(0));
        assert_eq!(parse_vmmap_size_to_bytes("1K"), Some(1024));
        assert_eq!(parse_vmmap_size_to_bytes("1M"), Some(1024 * 1024));
        assert_eq!(
            parse_vmmap_size_to_bytes("1.5M"),
            Some((1.5_f64 * 1024.0 * 1024.0).round() as u64)
        );
        assert_eq!(
            parse_vmmap_size_to_bytes("2G"),
            Some(2 * 1024 * 1024 * 1024)
        );
        assert_eq!(parse_vmmap_size_to_bytes("123"), Some(123));
    }

    #[test]
    fn parses_vmmap_summary_totals() {
        let sample = r#"
Physical footprint:         912.1M
Physical footprint (peak):  1.1G
===========                     ======= ========    =====  ======= ========   ======    =====  =======
TOTAL                              1.9G   233.4M    4113K       0K       0K       0K       0K     1529

MALLOC ZONE                         SIZE       SIZE       SIZE       SIZE      COUNT  ALLOCATED  FRAG SIZE  % FRAG   COUNT
===========                      =======  =========  =========  =========  =========  =========  =========  ======  ======
TOTAL                             950.0M      2464K      2464K         0K      10156       493K      1971K     81%      16
"#;

        let parsed = parse_vmmap_summary(sample);
        assert!(parsed.ok);
        assert_eq!(
            parsed.physical_footprint_bytes,
            parse_vmmap_size_to_bytes("912.1M")
        );
        assert_eq!(
            parsed.physical_footprint_peak_bytes,
            parse_vmmap_size_to_bytes("1.1G")
        );
        let total = parsed.total.expect("total");
        assert_eq!(
            total.virtual_bytes,
            parse_vmmap_size_to_bytes("1.9G").unwrap()
        );
        assert_eq!(
            total.resident_bytes,
            parse_vmmap_size_to_bytes("233.4M").unwrap()
        );
        assert_eq!(
            total.dirty_bytes,
            parse_vmmap_size_to_bytes("4113K").unwrap()
        );
        assert_eq!(total.swapped_bytes, 0);
        assert_eq!(total.region_count, 1529);

        let malloc = parsed.malloc_zone_total.expect("malloc_total");
        assert_eq!(
            malloc.virtual_bytes,
            parse_vmmap_size_to_bytes("950.0M").unwrap()
        );
        assert_eq!(
            malloc.resident_bytes,
            parse_vmmap_size_to_bytes("2464K").unwrap()
        );
        assert_eq!(
            malloc.allocated_bytes,
            parse_vmmap_size_to_bytes("493K").unwrap()
        );
        assert_eq!(malloc.frag_pct, 81.0);
        assert_eq!(malloc.region_count, 16);
    }

    #[test]
    fn parses_duration() {
        assert_eq!(parse_duration("250ms"), Some(Duration::from_millis(250)));
        assert_eq!(parse_duration("2s"), Some(Duration::from_secs(2)));
        assert_eq!(parse_duration("3m"), Some(Duration::from_secs(180)));
        assert_eq!(parse_duration("5"), Some(Duration::from_secs(5)));
    }
}
