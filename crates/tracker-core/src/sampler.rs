//! The sampling loop: reads the foreground window ~once per second and
//! persists segments of consecutive identical activity.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::Local;

use crate::category::{categorize, clean_title, is_browser};
use crate::db::Db;
use crate::model::{ActivityState, Category, Segment};
use crate::url::UrlExtractor;
use crate::win32;

#[derive(Debug, Clone)]
pub struct SamplerConfig {
    /// Time per tick.
    pub interval_ms: u64,
    /// After this much time without any input the PC counts as idle.
    pub idle_after_ms: u64,
    /// Try to read URLs from browser windows.
    pub track_urls: bool,
}

impl Default for SamplerConfig {
    fn default() -> Self {
        SamplerConfig {
            interval_ms: 1_000,
            idle_after_ms: 180_000, // 3 minutes
            track_urls: true,
        }
    }
}

pub struct Sampler {
    db: Db,
    cfg: SamplerConfig,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
}

impl Sampler {
    pub fn new(
        db: Db,
        cfg: SamplerConfig,
        stop: Arc<AtomicBool>,
        paused: Arc<AtomicBool>,
    ) -> Self {
        Sampler {
            db,
            cfg,
            stop,
            paused,
        }
    }

    /// Run the sampling loop until `stop` is set. Flushes the open segment
    /// on exit. Intended to be called from a dedicated thread.
    pub fn run(&mut self) -> Result<()> {
        let interval = Duration::from_millis(self.cfg.interval_ms.max(200));
        let mut exe_cache: HashMap<u32, Option<String>> = HashMap::new();
        let mut url_ext = if self.cfg.track_urls {
            Some(UrlExtractor::new())
        } else {
            None
        };
        let mut current: Option<Segment> = None;
        let mut last_meta_write = Instant::now();

        loop {
            let tick_start = Instant::now();

            if self.stop.load(Ordering::Relaxed) {
                Self::flush(&self.db, &mut current)?;
                self.write_meta(true)?;
                break;
            }

            // Cross-process control from other `tracker` invocations
            // (`tracker pause` / `resume` / `stop`). Cheap point queries.
            if let Ok(Some(v)) = self.db.meta_get("paused") {
                self.paused.store(v == "1", Ordering::Relaxed);
            }
            if let Ok(Some(v)) = self.db.meta_get("want_stop") {
                if v == "1" {
                    Self::flush(&self.db, &mut current)?;
                    let now = Local::now().timestamp_millis();
                    let _ = self.db.meta_set("want_stop", "0");
                    let _ = self.db.meta_set("last_flush_ms", &now.to_string());
                    let _ = self.db.meta_set("stopped_at", &now.to_string());
                    self.stop.store(true, Ordering::Relaxed);
                    break;
                }
            }

            let paused = self.paused.load(Ordering::Relaxed);
            if paused {
                Self::flush(&self.db, &mut current)?;
                // Keep the heartbeat alive so `status` still sees us.
                if last_meta_write.elapsed() >= Duration::from_secs(15) {
                    let _ = self.write_meta(false);
                    last_meta_write = Instant::now();
                }
                Self::sleep_until(tick_start + interval);
                continue;
            }

            let now_ms = Local::now().timestamp_millis();
            let idle_ms = win32::idle_millis();
            let state = if idle_ms >= self.cfg.idle_after_ms {
                ActivityState::Idle
            } else {
                ActivityState::Active
            };

            let (app, title, url) = match win32::foreground_window() {
                Some(win) => {
                    let app = match exe_cache.get(&win.pid) {
                        Some(cached) => cached.clone(),
                        None => {
                            let exe = win32::exe_name_of(win.pid);
                            if exe_cache.len() > 2048 {
                                exe_cache.clear();
                            }
                            exe_cache.insert(win.pid, exe.clone());
                            exe
                        }
                    };
                    let app = app.unwrap_or_else(|| format!("unknown-{}", win.pid));
                    let title = clean_title(&app, &win.title);
                    let url = if is_browser(&app) {
                        url_ext
                            .as_mut()
                            .and_then(|u| u.extract(win.hwnd, &win.title))
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    (app, title, url)
                }
                None => ("(none)".to_string(), String::new(), String::new()),
            };
            let category = categorize(&app);

            let end_ms = now_ms + interval.as_millis() as i64;
            match current {
                Some(ref mut seg)
                    if seg.app == app
                        && seg.title == title
                        && seg.url == url
                        && seg.state == state =>
                {
                    seg.end_ms = end_ms;
                }
                _ => {
                    Self::flush(&self.db, &mut current)?;
                    current = Some(Segment {
                        id: 0,
                        start_ms: now_ms,
                        end_ms,
                        app,
                        title,
                        url,
                        category,
                        state,
                    });
                }
            }

            // Heartbeat so `tracker status` can tell that watch is running.
            if last_meta_write.elapsed() >= Duration::from_secs(15) {
                Self::flush(&self.db, &mut current)?;
                self.write_meta(false)?;
                last_meta_write = Instant::now();
            }

            Self::sleep_until(tick_start + interval);
        }
        Ok(())
    }

    fn flush(db: &Db, current: &mut Option<Segment>) -> Result<()> {
        if let Some(seg) = current.take() {
            if seg.end_ms > seg.start_ms {
                db.insert_segments(std::slice::from_ref(&seg))?;
            }
        }
        Ok(())
    }

    fn write_meta(&self, final_flush: bool) -> Result<()> {
        let now = Local::now().timestamp_millis();
        self.db.meta_set("last_flush_ms", &now.to_string())?;
        if final_flush {
            self.db.meta_set("last_run_ended_ms", &now.to_string())?;
        }
        Ok(())
    }

    fn sleep_until(deadline: Instant) {
        let now = Instant::now();
        if deadline > now {
            std::thread::sleep(deadline - now);
        }
    }
}

/// Convenience: category of a segment string (used by tests/tools).
pub fn category_of(app: &str) -> Category {
    categorize(app)
}
