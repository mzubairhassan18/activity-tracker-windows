//! `tracker` — Windows activity tracker command line.

mod report_text;
mod tray;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono::{Duration as ChronoDuration, Local, NaiveDate, TimeZone};
use clap::{Parser, Subcommand};

use tracker_core::report::{day_bounds, load_day, load_week};
use tracker_core::{Db, Sampler, SamplerConfig};

#[derive(Parser)]
#[command(
    name = "tracker",
    version,
    about = "Windows activity tracker: records which apps you use, for how long, and what you were doing — with daily and weekly reports."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Record activity in the background (with a system tray icon).
    Watch {
        /// Seconds between samples.
        #[arg(long, default_value_t = 1.0)]
        interval: f32,
        /// Minutes without any keyboard/mouse input before time counts as idle.
        #[arg(long, default_value_t = 3.0)]
        idle_after: f32,
        /// Do not record browser URLs (titles are still recorded).
        #[arg(long)]
        no_urls: bool,
        /// Run without a tray icon (Ctrl+C stops it).
        #[arg(long)]
        no_tray: bool,
        /// Hide the console window (used by autostart).
        #[arg(long)]
        hidden: bool,
    },
    /// Show a text report for a day (default: today).
    Report {
        /// Show the whole current week instead of a single day.
        #[arg(long)]
        week: bool,
        /// Day to show: YYYY-MM-DD, "today" or "yesterday" (default: today).
        date: Option<String>,
    },
    /// Export raw segments to CSV or JSON.
    Export {
        /// First day, YYYY-MM-DD (default: 7 days ago).
        #[arg(long)]
        from: Option<String>,
        /// Last day, YYYY-MM-DD (default: today).
        #[arg(long)]
        to: Option<String>,
        /// Output format: csv or json.
        #[arg(long, default_value = "csv")]
        format: String,
        /// Output file, or "-" for stdout.
        #[arg(long, default_value = "-")]
        out: String,
    },
    /// Show whether tracking is running and where data is stored.
    Status,
    /// Pause recording (takes effect within a second; works on hidden instances).
    Pause,
    /// Resume recording after `tracker pause`.
    Resume,
    /// Gracefully stop a running `tracker watch` (flushes the current segment).
    Stop,
    /// Open the graphical dashboard window.
    Dashboard,
    /// Start/stop tracking automatically when you log in.
    Autostart {
        #[command(subcommand)]
        action: AutostartCmd,
    },
}

#[derive(Subcommand)]
enum AutostartCmd {
    On,
    Off,
    Status,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Watch {
            interval,
            idle_after,
            no_urls,
            no_tray,
            hidden,
        } => cmd_watch(interval, idle_after, no_urls, no_tray, hidden),
        Cmd::Report { week, date } => cmd_report(week, date),
        Cmd::Export {
            from,
            to,
            format,
            out,
        } => cmd_export(from, to, format, out),
        Cmd::Status => cmd_status(),
        Cmd::Pause => cmd_set_paused(true),
        Cmd::Resume => cmd_set_paused(false),
        Cmd::Stop => cmd_stop(),
        Cmd::Dashboard => {
            launch_dashboard()?;
            Ok(())
        }
        Cmd::Autostart { action } => cmd_autostart(action),
    }
}

// ---------------------------------------------------------------- watch ----

fn cmd_watch(interval: f32, idle_after: f32, no_urls: bool, no_tray: bool, hidden: bool) -> Result<()> {
    if hidden {
        tracker_core::win32::hide_console_window();
    }

    let db = Db::open()?;
    db.meta_set("want_stop", "0")?; // clear any stale stop request
    db.meta_set(
        "last_flush_ms",
        &chrono::Local::now().timestamp_millis().to_string(),
    )?; // heartbeat so status sees us right away
    let paused = Arc::new(AtomicBool::new(db.meta_get("paused")?.as_deref() == Some("1")));
    let stop = Arc::new(AtomicBool::new(false));
    let sampler_done = Arc::new(AtomicBool::new(false));

    let cfg = SamplerConfig {
        interval_ms: ((interval.max(0.2)) * 1000.0) as u64,
        idle_after_ms: ((idle_after.max(0.1)) * 60_000.0) as u64,
        track_urls: !no_urls,
    };
    let interval_ms = cfg.interval_ms;

    let sampler_db = Db::open()?;
    let stop_t = stop.clone();
    let paused_t = paused.clone();
    let done_t = sampler_done.clone();
    let sampler_thread = std::thread::spawn(move || {
        let mut sampler = Sampler::new(sampler_db, cfg, stop_t, paused_t);
        let result = sampler.run();
        done_t.store(true, Ordering::Relaxed);
        if let Err(e) = result {
            log_line(&format!("sampler stopped: {e:#}"));
        }
    });

    // Ctrl+C: ask the sampler to flush, then exit.
    {
        let stop = stop.clone();
        let _ = ctrlc::set_handler(move || {
            stop.store(true, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(interval_ms * 3 + 700));
            std::process::exit(0);
        });
    }

    if no_tray {
        // Run until Ctrl+C (the handler exits the process), an error in the
        // sampler, or `tracker stop`.
        while !sampler_done.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(500));
        }
        let _ = sampler_thread.join();
        return Ok(());
    }

    // Tray loop; returns only via ControlFlow::Exit after flushing.
    let db_for_pause = Db::open()?;
    tray::run(tray::TrayOptions {
        stop,
        paused,
        interval_ms,
        sampler_done,
        on_pause_change: Box::new(move |is_paused| {
            let _ = db_for_pause.meta_set("paused", if is_paused { "1" } else { "0" });
        }),
    })?;
    let _ = sampler_thread.join();
    Ok(())
}

// ------------------------------------------------------ pause / resume / stop ---

fn cmd_set_paused(paused: bool) -> Result<()> {
    let db = Db::open()?;
    db.meta_set("paused", if paused { "1" } else { "0" })?;
    // Give a running sampler a moment to notice (it checks every tick).
    std::thread::sleep(Duration::from_millis(1200));
    match db.last_activity_ms() {
        Some(ms) if Local::now().timestamp_millis() - ms < 60_000 => {
            println!(
                "Tracker is {}.",
                if paused { "paused" } else { "recording again" }
            );
        }
        _ => println!(
            "Marked {}; no active `tracker watch` process was seen recently.",
            if paused { "paused" } else { "resumed" }
        ),
    }
    Ok(())
}

fn cmd_stop() -> Result<()> {
    let db = Db::open()?;
    db.meta_set("want_stop", "1")?;
    // Wait for the sampler to flush and exit (checked every tick).
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(250));
        if db.meta_get("want_stop")?.as_deref() != Some("1") {
            println!("Tracker stopped (current segment flushed).");
            return Ok(());
        }
    }
    println!(
        "Stop requested — no running `tracker watch` acknowledged within10s.\n\
         (If nothing is running, the request is cleared on next start.)"
    );
    Ok(())
}

// --------------------------------------------------------------- report ----

fn parse_date(s: &str) -> Result<NaiveDate> {
    match s.to_lowercase().as_str() {
        "today" => Ok(Local::now().date_naive()),
        "yesterday" => Ok(Local::now().date_naive() - ChronoDuration::days(1)),
        _ => NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .with_context(|| format!("invalid date '{s}' (expected YYYY-MM-DD)")),
    }
}

fn cmd_report(week: bool, date: Option<String>) -> Result<()> {
    let db = Db::open()?;
    if week {
        let d = match date {
            Some(s) => parse_date(&s)?,
            None => Local::now().date_naive(),
        };
        let report = load_week(&db, d)?;
        report_text::render_week(&report);
        println!(
            "\nTip: detailed sessions for one day — `tracker report {}`",
            Local::now().date_naive()
        );
    } else {
        let d = match date {
            Some(s) => parse_date(&s)?,
            None => Local::now().date_naive(),
        };
        let report = load_day(&db, d)?;
        report_text::render_day(&report);
    }
    Ok(())
}

// --------------------------------------------------------------- export ----

fn cmd_export(from: Option<String>, to: Option<String>, format: String, out: String) -> Result<()> {
    let db = Db::open()?;
    let today = Local::now().date_naive();
    let from = match from {
        Some(s) => parse_date(&s)?,
        None => today - ChronoDuration::days(6),
    };
    let to = match to {
        Some(s) => parse_date(&s)?,
        None => today,
    };
    if to < from {
        bail!("'to' date is before 'from' date");
    }
    let (start, _) = day_bounds(from);
    let (_, end) = day_bounds(to + ChronoDuration::days(1));
    let segments = db.segments(start, end)?;

    let content = match format.as_str() {
        "csv" => {
            let mut csv = String::from("start,end,app,title,url,category,state,duration_seconds\n");
            for s in &segments {
                csv.push_str(&format!(
                    "{},{},{},{},{},{},{},{}\n",
                    iso(s.start_ms),
                    iso(s.end_ms),
                    csv_escape(&s.app),
                    csv_escape(&s.title),
                    csv_escape(&s.url),
                    s.category.as_str(),
                    s.state.as_str(),
                    s.duration_secs()
                ));
            }
            csv
        }
        "json" => serde_json::to_string_pretty(&segments)?,
        other => bail!("unknown format '{other}' (use csv or json)"),
    };

    if out == "-" {
        print!("{content}");
    } else {
        std::fs::write(&out, content.as_bytes())
            .with_context(|| format!("writing {out}"))?;
        println!(
            "exported {} segments ({} .. {}) to {}",
            segments.len(),
            from,
            to,
            out
        );
    }
    Ok(())
}

fn iso(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%S%:z").to_string())
        .unwrap_or_else(|| ms.to_string())
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

// --------------------------------------------------------------- status ----

fn cmd_status() -> Result<()> {
    let path = Db::db_path();
    println!("Database : {}", path.display());
    if !path.exists() {
        println!("Status   : no data yet — start tracking with `tracker watch`");
        return Ok(());
    }
    let db = Db::open()?;
    let count = db.segment_count()?;
    println!("Segments : {count}");

    match db.last_activity_ms() {
        Some(ms) => {
            let ago_ms = Local::now().timestamp_millis() - ms;
            let ago = (ago_ms.max(0) / 1000) as u64;
            let ago_str = if ago < 60 {
                format!("{ago}s ago")
            } else if ago < 3600 {
                format!("{}m ago", ago / 60)
            } else {
                format!("{}h ago", ago / 3600)
            };
            let running = ago_ms >= 0 && ago_ms < 60_000;
            println!(
                "Last seen : {ago_str} ({})",
                if running { "tracker is running" } else { "tracker not seen for a while" }
            );
        }
        None => println!("Last seen : never"),
    }
    let paused = db.meta_get("paused")?.as_deref() == Some("1");
    println!("Paused    : {}", if paused { "yes" } else { "no" });

    let today = load_day(&db, Local::now().date_naive())?;
    println!(
        "Today     : {} active, {} idle",
        report_text::fmt_dur(today.active_secs),
        report_text::fmt_dur(today.idle_secs)
    );
    Ok(())
}

// ---------------------------------------------------------- diagnostics ---

/// Where the tracker keeps its data; reused for the log file next to it.
fn data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("activity-tracker")
}

/// Append a line to `%LOCALAPPDATA%\activity-tracker\tracker.log`.
///
/// With `--hidden` the console window is gone, so anything printed to stderr is
/// invisible: a tray action that fails would otherwise do nothing at all.
pub(crate) fn log_line(msg: &str) {
    let dir = data_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("tracker.log"))
    {
        let ts = Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}

// ------------------------------------------------------------ dashboard ----

pub(crate) fn launch_dashboard() -> Result<()> {
    let exe = std::env::current_exe().context("locating own executable")?;
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));
    let candidate = dir.join("tracker-dashboard.exe");
    if candidate.exists() {
        Command::new(&candidate)
            .spawn()
            .with_context(|| format!("launching {}", candidate.display()))?;
        return Ok(());
    }
    bail!(
        "tracker-dashboard.exe not found next to {} — build it with `cargo build -p tracker-dashboard`",
        exe.display()
    );
}

// ------------------------------------------------------------- autostart ---

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "ActivityTracker";

fn cmd_autostart(action: AutostartCmd) -> Result<()> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    match action {
        AutostartCmd::On => {
            let exe = std::env::current_exe().context("locating own executable")?;
            let cmd = format!("\"{}\" watch --hidden", exe.display());
            let (key, _) = hkcu
                .create_subkey_with_flags(RUN_KEY, KEY_READ | KEY_WRITE)
                .context("opening HKCU Run key")?;
            key.set_value(RUN_VALUE, &cmd)
                .context("writing autostart entry")?;
            println!("Autostart enabled: {cmd}");
        }
        AutostartCmd::Off => {
            let (key, _) = hkcu
                .create_subkey_with_flags(RUN_KEY, KEY_READ | KEY_WRITE)
                .context("opening HKCU Run key")?;
            match key.delete_value(RUN_VALUE) {
                Ok(()) => println!("Autostart disabled."),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    println!("Autostart was not enabled.")
                }
                Err(e) => return Err(e).context("removing autostart entry"),
            }
        }
        AutostartCmd::Status => {
            let key = hkcu
                .open_subkey_with_flags(RUN_KEY, KEY_READ)
                .context("opening HKCU Run key")?;
            match key.get_value::<String, _>(RUN_VALUE) {
                Ok(cmd) => println!("Autostart enabled:\n  {cmd}"),
                Err(_) => println!("Autostart is disabled."),
            }
        }
    }
    Ok(())
}
