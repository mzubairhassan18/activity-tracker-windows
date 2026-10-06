//! Aggregation of raw segments into day/week reports.

use std::collections::HashMap;

use anyhow::Result;
use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone, Weekday};

use crate::db::Db;
use crate::model::{ActivityState, Category, Segment};

/// Time spent in one application (active time only).
#[derive(Debug, Clone)]
pub struct AppTime {
    pub app: String,
    pub app_name: String,
    pub secs: u64,
}

/// Time spent in one category (active time only).
#[derive(Debug, Clone)]
pub struct CategoryTime {
    pub category: Category,
    pub secs: u64,
}

/// Report for a single local day.
#[derive(Debug, Clone)]
pub struct DayReport {
    pub date: NaiveDate,
    pub active_secs: u64,
    pub idle_secs: u64,
    /// Sorted descending by active time.
    pub apps: Vec<AppTime>,
    /// Sorted descending by time.
    pub categories: Vec<CategoryTime>,
    /// Segments clipped to the day, in chronological order (includes idle).
    pub sessions: Vec<Segment>,
}

impl DayReport {
    pub fn build(date: NaiveDate, segments: &[Segment]) -> Self {
        let (ds, de) = day_bounds(date);

        // Coalesce contiguous fragments of the same activity. The sampler
        // flushes every ~15s as a crash-safety heartbeat, which would
        // otherwise show up as a burst of tiny sessions.
        let mut merged: Vec<Segment> = Vec::with_capacity(segments.len());
        for seg in segments {
            if let Some(last) = merged.last_mut() {
                let same_key = last.app == seg.app
                    && last.title == seg.title
                    && last.url == seg.url
                    && last.state == seg.state
                    && last.category == seg.category;
                let contiguous = seg.start_ms >= last.start_ms
                    && seg.start_ms.saturating_sub(last.end_ms) <= 1;
                if same_key && contiguous {
                    last.end_ms = last.end_ms.max(seg.end_ms);
                    continue;
                }
            }
            merged.push(seg.clone());
        }

        let mut active_secs = 0u64;
        let mut idle_secs = 0u64;
        let mut app_secs: HashMap<&str, u64> = HashMap::new();
        let mut cat_secs: HashMap<Category, u64> = HashMap::new();
        let mut sessions = Vec::new();

        for seg in &merged {
            let overlap = seg.overlap_ms(ds, de);
            if overlap <= 0 {
                continue;
            }
            let secs = (overlap / 1000) as u64;
            match seg.state {
                ActivityState::Active => {
                    active_secs += secs;
                    *app_secs.entry(seg.app.as_str()).or_insert(0) += secs;
                    *cat_secs.entry(seg.category).or_insert(0) += secs;
                }
                ActivityState::Idle => idle_secs += secs,
            }
            let mut clipped = seg.clone();
            clipped.start_ms = seg.start_ms.max(ds);
            clipped.end_ms = seg.end_ms.min(de);
            sessions.push(clipped);
        }

        let mut apps: Vec<AppTime> = app_secs
            .into_iter()
            .map(|(app, secs)| AppTime {
                app_name: crate::category::friendly_name(app),
                app: app.to_string(),
                secs,
            })
            .collect();
        apps.sort_by(|a, b| b.secs.cmp(&a.secs).then_with(|| a.app.cmp(&b.app)));

        let mut categories: Vec<CategoryTime> = cat_secs
            .into_iter()
            .map(|(category, secs)| CategoryTime { category, secs })
            .collect();
        categories.sort_by(|a, b| b.secs.cmp(&a.secs));

        sessions.sort_by_key(|s| s.start_ms);

        DayReport {
            date,
            active_secs,
            idle_secs,
            apps,
            categories,
            sessions,
        }
    }
}

/// Report for a week (7 local days, Monday first).
#[derive(Debug, Clone)]
pub struct WeekReport {
    /// Monday of the week.
    pub start: NaiveDate,
    pub days: Vec<DayReport>,
}

impl WeekReport {
    pub fn build(start: NaiveDate, segments: &[Segment]) -> Self {
        let days = (0..7)
            .map(|i| {
                let date = start + Duration::days(i);
                DayReport::build(date, segments)
            })
            .collect();
        WeekReport { start, days }
    }

    pub fn active_secs(&self) -> u64 {
        self.days.iter().map(|d| d.active_secs).sum()
    }

    pub fn idle_secs(&self) -> u64 {
        self.days.iter().map(|d| d.idle_secs).sum()
    }

    /// Active time per application across the week, sorted descending.
    pub fn apps(&self) -> Vec<AppTime> {
        let mut map: HashMap<&str, (String, u64)> = HashMap::new();
        for day in &self.days {
            for a in &day.apps {
                let e = map
                    .entry(a.app.as_str())
                    .or_insert_with(|| (a.app.clone(), 0));
                e.1 += a.secs;
            }
        }
        let mut out: Vec<AppTime> = map
            .into_values()
            .map(|(app, secs)| AppTime {
                app_name: crate::category::friendly_name(&app),
                app,
                secs,
            })
            .collect();
        out.sort_by(|a, b| b.secs.cmp(&a.secs).then_with(|| a.app.cmp(&b.app)));
        out
    }

    /// Active time per category across the week, sorted descending.
    pub fn categories(&self) -> Vec<CategoryTime> {
        let mut map: HashMap<Category, u64> = HashMap::new();
        for day in &self.days {
            for c in &day.categories {
                *map.entry(c.category).or_insert(0) += c.secs;
            }
        }
        let mut out: Vec<CategoryTime> = map
            .into_iter()
            .map(|(category, secs)| CategoryTime { category, secs })
            .collect();
        out.sort_by(|a, b| b.secs.cmp(&a.secs));
        out
    }
}

/// Local-time bounds [start_ms, end_ms) of a calendar day.
pub fn day_bounds(date: NaiveDate) -> (i64, i64) {
    (timestamp_at(date), timestamp_at(date + Duration::days(1)))
}

/// Monday of the week containing `date`.
pub fn week_start(date: NaiveDate) -> NaiveDate {
    let offset = match date.weekday() {
        Weekday::Mon => 0,
        Weekday::Tue => 1,
        Weekday::Wed => 2,
        Weekday::Thu => 3,
        Weekday::Fri => 4,
        Weekday::Sat => 5,
        Weekday::Sun => 6,
    };
    date - Duration::days(offset)
}

/// Load and build the report for one day.
pub fn load_day(db: &Db, date: NaiveDate) -> Result<DayReport> {
    let (ds, de) = day_bounds(date);
    let segments = db.segments(ds, de)?;
    Ok(DayReport::build(date, &segments))
}

/// Load and build the report for the week (Monday..Sunday) containing `date`.
pub fn load_week(db: &Db, date: NaiveDate) -> Result<WeekReport> {
    let start = week_start(date);
    let (ws, _) = day_bounds(start);
    let (_, we) = day_bounds(start + Duration::days(7));
    let segments = db.segments(ws, we)?;
    Ok(WeekReport::build(start, &segments))
}

/// Unix ms for local midnight of `date`, tolerating DST gaps.
fn timestamp_at(date: NaiveDate) -> i64 {
    for hour in 0..4 {
        if let Some(ndt) = date.and_hms_opt(hour, 0, 0) {
            if let Some(dt) = Local.from_local_datetime(&ndt).earliest() {
                return dt.timestamp_millis();
            }
        }
    }
    // Pathological fallback (no local midnight in the first hours).
    date.and_hms_opt(0, 0, 0)
        .unwrap()
        .and_local_timezone(Local)
        .unwrap()
        .timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn week_starts_on_monday() {
        // 2026-10-06 is a Tuesday.
        let d = NaiveDate::from_ymd_opt(2026, 10, 6).unwrap();
        assert_eq!(week_start(d), NaiveDate::from_ymd_opt(2026, 10, 5).unwrap());
        let mon = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
        assert_eq!(week_start(mon), mon);
        let sun = NaiveDate::from_ymd_opt(2026, 10, 11).unwrap();
        assert_eq!(week_start(sun), mon);
    }

    #[test]
    fn day_aggregation() {
        let date = NaiveDate::from_ymd_opt(2026, 10, 6).unwrap();
        let (s, e) = day_bounds(date);
        let seg = Segment {
            id: 1,
            start_ms: s + 3_600_000,
            end_ms: s + 7_200_000,
            app: "code.exe".into(),
            title: "main.rs".into(),
            url: String::new(),
            category: Category::Development,
            state: ActivityState::Active,
        };
        let report = DayReport::build(date, std::slice::from_ref(&seg));
        assert_eq!(report.active_secs, 3600);
        assert_eq!(report.idle_secs, 0);
        assert_eq!(report.apps.len(), 1);
        assert_eq!(report.apps[0].app_name, "Visual Studio Code");
        assert_eq!(report.sessions.len(), 1);
        assert!(report.sessions[0].end_ms <= e);
    }

    #[test]
    fn merges_contiguous_fragments() {
        let date = NaiveDate::from_ymd_opt(2026, 10, 6).unwrap();
        let (s, _) = day_bounds(date);
        let make = |start: i64, end: i64| Segment {
            id: 0,
            start_ms: s + start,
            end_ms: s + end,
            app: "code.exe".into(),
            title: "main.rs".into(),
            url: String::new(),
            category: Category::Development,
            state: ActivityState::Active,
        };
        // Three heartbeat fragments of one continuous session.
        let segs = vec![
            make(3_600_000, 4_500_000),
            make(4_500_000, 5_400_000),
            make(5_400_000, 6_300_000),
        ];
        let report = DayReport::build(date, &segs);
        assert_eq!(report.sessions.len(), 1, "fragments should merge");
        assert_eq!(report.sessions[0].duration_secs(), 2700);
        assert_eq!(report.active_secs, 2700);

        // A gap (different activity in between) must NOT merge.
        let other = Segment {
            id: 0,
            start_ms: s + 4_500_000,
            end_ms: s + 5_400_000,
            app: "chrome.exe".into(),
            title: "other".into(),
            url: String::new(),
            category: Category::Browsing,
            state: ActivityState::Active,
        };
        let segs = vec![make(3_600_000, 4_500_000), other, make(5_400_000, 6_300_000)];
        let report = DayReport::build(date, &segs);
        assert_eq!(report.sessions.len(), 3, "different activity breaks merge");
    }
}
