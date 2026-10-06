//! Plain-text rendering of day/week reports for the terminal.

use chrono::NaiveDate;

use tracker_core::report::{AppTime, CategoryTime, DayReport, WeekReport};
use tracker_core::Segment;

/// "6h 12m", "42m", "55s"
pub fn fmt_dur(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

/// Left-align in a fixed width.
fn pad(s: &str, width: usize) -> String {
    let mut out = String::from(s);
    while out.chars().count() < width {
        out.push(' ');
    }
    out
}

/// Right-align in a fixed width.
fn pad_left(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len >= width {
        s.to_string()
    } else {
        format!("{}{}", " ".repeat(width - len), s)
    }
}

fn bar(secs: u64, max: u64, width: usize) -> String {
    if max == 0 {
        return " ".repeat(width);
    }
    let filled = ((secs as f64 / max as f64) * width as f64).round() as usize;
    let filled = filled.min(width).max(1);
    format!("{}{}", "#".repeat(filled), " ".repeat(width - filled))
}

fn pct(part: u64, whole: u64) -> u64 {
    if whole == 0 {
        0
    } else {
        (part as f64 / whole as f64 * 100.0).round() as u64
    }
}

fn print_apps(apps: &[AppTime], total_active: u64) {
    println!("\nApplications (active time)");
    if apps.is_empty() {
        println!("  (no data)");
        return;
    }
    let max = apps[0].secs;
    for a in apps.iter().take(15) {
        println!(
            "  {}  {}  [{}]  {}%",
            pad_left(&fmt_dur(a.secs), 8),
            pad(&a.app_name, 24),
            bar(a.secs, max, 24),
            pct(a.secs, total_active)
        );
    }
    if apps.len() > 15 {
        println!("  ... and {} more applications", apps.len() - 15);
    }
}

fn print_categories(cats: &[CategoryTime], total_active: u64) {
    println!("\nBy category");
    if cats.is_empty() {
        println!("  (no data)");
        return;
    }
    let max = cats[0].secs;
    for c in cats {
        println!(
            "  {}  {}  [{}]",
            pad_left(&fmt_dur(c.secs), 8),
            pad(c.category.label(), 24),
            bar(c.secs, max, 24)
        );
    }
    let _ = total_active;
}

fn print_sessions(sessions: &[Segment], day_secs: i64) {
    println!("\nSessions");
    if sessions.is_empty() {
        println!("  (no data)");
        return;
    }
    const MAX: usize = 200;
    let mut shown = 0usize;
    for seg in sessions {
        if shown >= MAX {
            println!("  ... and {} more sessions", sessions.len() - shown);
            break;
        }
        let start = time_of_day(seg.start_ms, day_secs);
        let end = time_of_day(seg.end_ms, day_secs);
        let dur = fmt_dur(seg.duration_secs());
        let app = tracker_core::category::friendly_name(&seg.app);
        let label = match seg.state {
            tracker_core::ActivityState::Idle => "— idle —".to_string(),
            tracker_core::ActivityState::Active => {
                if seg.title.is_empty() {
                    app
                } else {
                    truncate(&seg.title, 56)
                }
            }
        };
        let idle_marker = if seg.state == tracker_core::ActivityState::Idle {
            " (idle)"
        } else {
            ""
        };
        println!(
            "  {}–{}  {}  {}{}",
            pad(&start, 5),
            pad(&end, 5),
            pad_left(&dur, 7),
            truncate(&label, 60),
            idle_marker
        );
        shown += 1;
    }
}

fn time_of_day(ms: i64, day_secs: i64) -> String {
    let within = (ms - day_secs).max(0) / 1000;
    let within = within.min(86_399);
    format!("{:02}:{:02}", within / 3600, (within % 3600) / 60)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

/// Full report for one day.
pub fn render_day(report: &DayReport) {
    let (day_start, day_end) = tracker_core::report::day_bounds(report.date);
    let weekday = report.date.format("%A").to_string();
    let date_str = report.date.format("%Y-%m-%d").to_string();
    println!("Activity — {weekday}, {date_str}");
    println!(
        "  Active: {}   Idle: {}",
        fmt_dur(report.active_secs),
        fmt_dur(report.idle_secs)
    );
    print_apps(&report.apps, report.active_secs);
    print_categories(&report.categories, report.active_secs);
    print_sessions(&report.sessions, day_start);
    let _ = day_end;
}

/// Full report for one week (Monday..Sunday).
pub fn render_week(report: &WeekReport) {
    let end = report.start + chrono::Duration::days(6);
    println!(
        "Week of {} (Mon) – {} (Sun)",
        report.start.format("%Y-%m-%d"),
        end.format("%Y-%m-%d")
    );
    println!("\n{:<10} {:>9} {:>9}", "Day", "Active", "Idle");
    for day in &report.days {
        let label = day_label(day.date);
        println!(
            "{:<10} {:>9} {:>9}",
            label,
            fmt_dur(day.active_secs),
            fmt_dur(day.idle_secs)
        );
    }
    println!(
        "{:<10} {:>9} {:>9}",
        "Total",
        fmt_dur(report.active_secs()),
        fmt_dur(report.idle_secs())
    );
    print_apps(&report.apps(), report.active_secs());
    print_categories(&report.categories(), report.active_secs());
}

fn day_label(date: NaiveDate) -> String {
    let name = date.format("%a").to_string();
    format!("{} {}", name, date.format("%d %b"))
}
