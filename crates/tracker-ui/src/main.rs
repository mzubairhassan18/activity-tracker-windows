//! Graphical weekly dashboard for the activity tracker.

use std::time::{Duration, Instant};

use chrono::{Datelike, Duration as ChronoDuration, Local, NaiveDate, NaiveTime};
use eframe::egui::{
    self, Align, Color32, FontId, Layout, Pos2, Rect, RichText, Sense, Stroke, Vec2,
};

use tracker_core::report::{day_bounds, load_week, WeekReport};
use tracker_core::{ActivityState, Category, Db, Segment};

const REFRESH_EVERY: Duration = Duration::from_secs(30);

fn cat_color(c: Category) -> Color32 {
    match c {
        Category::Development => Color32::from_rgb(59, 130, 246),
        Category::Browsing => Color32::from_rgb(16, 185, 129),
        Category::Communication => Color32::from_rgb(245, 158, 11),
        Category::Office => Color32::from_rgb(139, 92, 246),
        Category::Media => Color32::from_rgb(236, 72, 153),
        Category::System => Color32::from_rgb(100, 116, 139),
        Category::Other => Color32::from_rgb(148, 163, 184),
    }
}

const IDLE_COLOR: Color32 = Color32::from_rgb(71, 85, 105);

fn fmt_dur(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m")
    } else {
        format!("{secs}s")
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

struct App {
    db: Db,
    week: Option<WeekReport>,
    error: Option<String>,
    /// Index (0..7) of the selected day within the displayed week.
    selected: usize,
    /// Any date inside the displayed week.
    anchor: NaiveDate,
    last_reload: Instant,
}

impl App {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let today = Local::now().date_naive();
        let (db, mut db_error) = match Db::open() {
            Ok(db) => (db, None),
            Err(e) => {
                let fallback = std::env::temp_dir().join("tracker-dashboard-fallback.db");
                match Db::open_at(&fallback) {
                    Ok(db) => (db, Some(format!("main database unavailable: {e:#}"))),
                    Err(e2) => panic!("cannot open tracker database: {e2:#}"),
                }
            }
        };
        let mut app = App {
            db,
            week: None,
            error: db_error.take(),
            selected: 0,
            anchor: today,
            last_reload: Instant::now() - REFRESH_EVERY,
        };
        app.reload();
        // Select today when it falls in the loaded week.
        if let Some(w) = &app.week {
            let today = Local::now().date_naive();
            if today >= w.start && today < w.start + ChronoDuration::days(7) {
                app.selected = (today - w.start).num_days() as usize;
            } else {
                app.selected = 6;
            }
        }
        app
    }

    fn reload(&mut self) {
        self.last_reload = Instant::now();
        match load_week(&self.db, self.anchor) {
            Ok(w) => {
                self.error = None;
                self.week = Some(w);
            }
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }

    fn shift_week(&mut self, days: i64) {
        self.anchor += ChronoDuration::days(days);
        self.reload();
        let today = Local::now().date_naive();
        if let Some(w) = &self.week {
            if today >= w.start && today < w.start + ChronoDuration::days(7) {
                self.selected = (today - w.start).num_days() as usize;
            } else {
                self.selected = 6;
            }
        }
    }

    fn go_today(&mut self) {
        self.anchor = Local::now().date_naive();
        self.reload();
        if let Some(w) = &self.week {
            let today = Local::now().date_naive();
            if today >= w.start && today < w.start + ChronoDuration::days(7) {
                self.selected = (today - w.start).num_days() as usize;
            }
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.last_reload.elapsed() > REFRESH_EVERY {
            self.reload();
        }
        ctx.request_repaint_after(Duration::from_secs(15));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                if ui.button("← Prev week").clicked() {
                    self.shift_week(-7);
                }
                if ui.button("Next week →").clicked() {
                    self.shift_week(7);
                }
                if ui.button("Today").clicked() {
                    self.go_today();
                }
                if ui.button("Refresh").clicked() {
                    self.reload();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(w) = &self.week {
                        ui.label(
                            RichText::new(format!(
                                "Total active this week: {}",
                                fmt_dur(w.active_secs())
                            ))
                            .strong(),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{} – {}",
                                w.start,
                                w.start + ChronoDuration::days(6)
                            ))
                            .weak(),
                        );
                    }
                });
            });
            ui.separator();

            if let Some(err) = &self.error {
                ui.colored_label(Color32::RED, format!("Error: {err}"));
            }
            // Take the report out of self so views can use &mut self next to
            // a borrow of it (and avoid a deep clone every frame).
            let week = self.week.take();
            let Some(week_ref) = week.as_ref() else {
                ui.label("No data loaded yet.");
                return;
            };
            let selected = self.selected;

            egui::ScrollArea::vertical().show(ui, |ui| {
                self.week_strip(ui, week_ref, selected);
                ui.separator();

                let day = &week_ref.days[selected];
                self.day_header(ui, day);
                self.timeline(ui, day);
                ui.separator();

                self.apps_and_categories(ui, day);
                ui.separator();

                self.sessions(ui, day);
            });

            self.week = week;
        });
    }
}

// ----------------------------------------------------------------- views ---

impl App {
    /// Seven vertical bars (active + idle stacked); click to select a day.
    fn week_strip(&mut self, ui: &mut egui::Ui, week: &WeekReport, selected: usize) {
        let available = ui.available_size();
        let height = 170.0f32.min(available.y.max(120.0));
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(available.x, height), Sense::click());
        let painter = ui.painter_at(rect);

        let label_h = 34.0;
        let plot = Rect::from_min_max(
            Pos2::new(rect.left(), rect.top()),
            Pos2::new(rect.right(), rect.bottom() - label_h),
        );
        painter.rect_filled(plot, 0.0, Color32::from_gray(24));

        let max = week
            .days
            .iter()
            .map(|d| (d.active_secs + d.idle_secs).max(1))
            .max()
            .unwrap_or(1)
            .max(3600);
        // Gridlines every 2 hours of the maximum.
        let step = if max > 8 * 3600 {
            2 * 3600
        } else if max > 4 * 3600 {
            3600
        } else {
            1800
        };
        let mut g = step;
        while g < max {
            let y = plot.bottom() - (g as f32 / max as f32) * plot.height();
            painter.line_segment(
                [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
                Stroke::new(1.0, Color32::from_gray(40)),
            );
            painter.text(
                Pos2::new(plot.left() + 4.0, y - 14.0),
                egui::Align2::LEFT_TOP,
                fmt_dur(g as u64),
                FontId::proportional(10.0),
                Color32::from_gray(110),
            );
            g += step;
        }

        let slot = plot.width() / 7.0;
        let hover_pos = resp.hover_pos().or(ui.input(|i| i.pointer.hover_pos()));
        let mut hover_text = String::new();

        for (i, day) in week.days.iter().enumerate() {
            let x0 = plot.left() + i as f32 * slot;
            let bar_w = slot * 0.56;
            let bx = x0 + (slot - bar_w) / 2.0;

            let active_h = (day.active_secs as f32 / max as f32) * plot.height();
            let idle_h = (day.idle_secs as f32 / max as f32) * plot.height();

            let is_selected = i == selected;
            let hovered = hover_pos
                .map(|p| p.x >= x0 && p.x < x0 + slot)
                .unwrap_or(false);

            if hovered {
                hover_text = format!(
                    "{} — {} active, {} idle\n{} sessions — click to inspect",
                    day.date.format("%A %Y-%m-%d"),
                    fmt_dur(day.active_secs),
                    fmt_dur(day.idle_secs),
                    day.sessions.len()
                );
            }

            // Idle on top of active.
            let active_rect = Rect::from_min_max(
                Pos2::new(bx, plot.bottom() - active_h),
                Pos2::new(bx + bar_w, plot.bottom()),
            );
            let idle_rect = Rect::from_min_max(
                Pos2::new(bx, active_rect.top() - idle_h),
                Pos2::new(bx + bar_w, active_rect.top()),
            );
            let active_color = if is_selected {
                cat_color(Category::Development)
            } else if hovered {
                Color32::from_rgb(96, 165, 250)
            } else {
                Color32::from_rgb(37, 99, 235)
            };
            painter.rect_filled(active_rect, 2.0, active_color);
            if day.idle_secs > 0 {
                painter.rect_filled(idle_rect, 2.0, IDLE_COLOR);
            }
            if is_selected {
                painter.rect_stroke(
                    Rect::from_min_max(
                        Pos2::new(bx - 3.0, active_rect.top().min(idle_rect.top()) - 3.0),
                        Pos2::new(bx + bar_w + 3.0, plot.bottom() + 3.0),
                    ),
                    2.0,
                    Stroke::new(2.0, Color32::WHITE),
                    egui::StrokeKind::Outside,
                );
            }

            // Labels under the bar.
            let day_label = day.date.format("%a %d").to_string();
            let cx = x0 + slot / 2.0;
            painter.text(
                Pos2::new(cx, plot.bottom() + 4.0),
                egui::Align2::CENTER_TOP,
                day_label,
                FontId::proportional(12.0),
                if is_selected {
                    Color32::WHITE
                } else {
                    Color32::from_gray(200)
                },
            );
            painter.text(
                Pos2::new(cx, plot.bottom() + 19.0),
                egui::Align2::CENTER_TOP,
                fmt_dur(day.active_secs),
                FontId::proportional(11.0),
                Color32::from_gray(150),
            );
        }

        if resp.clicked() {
            if let Some(p) = resp.hover_pos() {
                let idx = (((p.x - plot.left()) / slot).floor() as i64).clamp(0, 6);
                self.selected = idx as usize;
            }
        }
        if !hover_text.is_empty() {
            resp.on_hover_text(hover_text);
        }
    }

    fn day_header(&self, ui: &mut egui::Ui, day: &tracker_core::DayReport) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} — {}",
                    day.date.format("%A, %B %d %Y"),
                    if day.date == Local::now().date_naive() {
                        "today"
                    } else {
                        ""
                    }
                ))
                .heading(),
            );
            ui.label(
                RichText::new(format!("active {}", fmt_dur(day.active_secs)))
                    .color(Color32::from_rgb(96, 165, 250))
                    .strong(),
            );
            ui.label(
                RichText::new(format!("idle {}", fmt_dur(day.idle_secs)))
                    .color(IDLE_COLOR),
            );
        });
    }

    /// 00:00–24:00 strip with coloured segments; hover shows details.
    fn timeline(&self, ui: &mut egui::Ui, day: &tracker_core::DayReport) {
        let available = ui.available_size();
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(available.x, 56.0), Sense::hover());
        let painter = ui.painter_at(rect);

        let top = rect.top() + 14.0;
        let bottom = rect.bottom() - 14.0;
        let strip = Rect::from_min_max(Pos2::new(rect.left(), top), Pos2::new(rect.right(), bottom));
        painter.rect_filled(strip, 0.0, Color32::from_gray(24));

        let (ds, de) = day_bounds(day.date);
        let span = (de - ds) as f32;
        let w = strip.width();

        let mut hover_text: Option<String> = None;
        let hover_x = resp.hover_pos().map(|p| p.x);

        for seg in &day.sessions {
            let x0 = strip.left() + ((seg.start_ms - ds) as f32 / span) * w;
            let x1 = strip.left() + ((seg.end_ms - ds) as f32 / span) * w;
            let x1 = x1.max(x0 + 1.0);
            let color = match seg.state {
                ActivityState::Idle => IDLE_COLOR,
                ActivityState::Active => cat_color(seg.category),
            };
            let r = Rect::from_min_max(Pos2::new(x0, top), Pos2::new(x1, bottom));
            painter.rect_filled(r, 1.0, color);

            if let Some(hx) = hover_x {
                if hx >= x0 && hx < x1 {
                    let app = tracker_core::category::friendly_name(&seg.app);
                    let mut text = format!(
                        "{} – {}  ({})\n{}",
                        time_str(seg.start_ms),
                        time_str(seg.end_ms),
                        fmt_dur(seg.duration_secs()),
                        app
                    );
                    if !seg.title.is_empty() {
                        text.push('\n');
                        text.push_str(&truncate(&seg.title, 80));
                    }
                    if !seg.url.is_empty() {
                        text.push('\n');
                        text.push_str(&truncate(&seg.url, 80));
                    }
                    if seg.state == ActivityState::Idle {
                        text.push_str("\n(idle — no input)");
                    }
                    hover_text = Some(text);
                }
            }
        }

        // Hour ticks + labels.
        for h in (0..=24).step_by(if w > 900.0 { 1 } else { 2 }) {
            let x = strip.left() + (h as f32 / 24.0) * w;
            painter.line_segment(
                [Pos2::new(x, strip.bottom()), Pos2::new(x, strip.bottom() + 4.0)],
                Stroke::new(1.0, Color32::from_gray(90)),
            );
            if h % 3 == 0 && h < 24 {
                painter.text(
                    Pos2::new(x + 2.0, strip.bottom() + 3.0),
                    egui::Align2::LEFT_TOP,
                    format!("{h:02}:00"),
                    FontId::proportional(10.0),
                    Color32::from_gray(130),
                );
            }
        }
        painter.text(
            Pos2::new(strip.left() + 4.0, top - 13.0),
            egui::Align2::LEFT_TOP,
            "Activity timeline",
            FontId::proportional(11.0),
            Color32::from_gray(150),
        );

        if let Some(t) = hover_text {
            resp.on_hover_text(t);
        }
    }

    fn apps_and_categories(&self, ui: &mut egui::Ui, day: &tracker_core::DayReport) {
        ui.columns(2, |columns| {
            // Applications.
            columns[0].heading("Applications");
            if day.apps.is_empty() {
                columns[0].label("(no active time recorded)");
            } else {
                let max = day.apps.first().map(|a| a.secs).unwrap_or(1).max(1);
                for a in &day.apps {
                    columns[0].horizontal(|ui| {
                        ui.label(format!("{}", a.app_name));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(fmt_dur(a.secs)).strong());
                        });
                    });
                    let frac = a.secs as f32 / max as f32;
                    columns[0].add(
                        egui::ProgressBar::new(frac)
                            .desired_height(8.0)
                            .fill(cat_color(Category::Development)),
                    );
                }
            }

            // Categories.
            columns[1].heading("Categories");
            if day.categories.is_empty() {
                columns[1].label("(no active time recorded)");
            } else {
                let max = day.categories.first().map(|c| c.secs).unwrap_or(1).max(1);
                for c in &day.categories {
                    columns[1].horizontal(|ui| {
                        ui.colored_label(cat_color(c.category), "•");
                        ui.label(c.category.label());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(fmt_dur(c.secs)).strong());
                        });
                    });
                    let frac = c.secs as f32 / max as f32;
                    columns[1].add(
                        egui::ProgressBar::new(frac)
                            .desired_height(8.0)
                            .fill(cat_color(c.category)),
                    );
                }
            }
        });
    }

    fn sessions(&self, ui: &mut egui::Ui, day: &tracker_core::DayReport) {
        ui.horizontal(|ui| {
            ui.heading("What you did");
            ui.label(
                RichText::new(format!("{} segments", day.sessions.len()))
                    .weak(),
            );
        });

        if day.sessions.is_empty() {
            ui.label("Nothing recorded for this day.");
            return;
        }

        egui::ScrollArea::vertical()
            .id_salt("sessions")
            .max_height(320.0)
            .show(ui, |ui| {
                for seg in &day.sessions {
                    ui.horizontal(|ui| {
                        let color = match seg.state {
                            ActivityState::Idle => IDLE_COLOR,
                            ActivityState::Active => cat_color(seg.category),
                        };
                        ui.colored_label(color, "•");
                        ui.label(
                            RichText::new(format!(
                                "{}–{}",
                                time_str(seg.start_ms),
                                time_str(seg.end_ms)
                            ))
                            .monospace(),
                        );
                        ui.label(RichText::new(fmt_dur(seg.duration_secs())).weak());

                        if seg.state == ActivityState::Idle {
                            ui.label(RichText::new("idle — away from keyboard").italics());
                        } else {
                            let app = tracker_core::category::friendly_name(&seg.app);
                            ui.label(RichText::new(&app).strong());
                            let detail = match (seg.title.is_empty(), seg.url.is_empty()) {
                                (false, false) => format!("{} — {}", seg.title, seg.url),
                                (false, true) => seg.title.clone(),
                                (true, false) => seg.url.clone(),
                                (true, true) => String::new(),
                            };
                            if !detail.is_empty() {
                                ui.label(RichText::new(truncate(&detail, 90)).weak());
                            }
                        }
                    });
                }
            });
    }
}

fn time_str(ms: i64) -> String {
    let dt = chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| d.with_timezone(&Local))
        .unwrap_or_else(Local::now);
    let t: NaiveTime = dt.time();
    t.format("%H:%M").to_string()
}

// ------------------------------------------------------------------ main ---

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Activity Tracker — weekly dashboard")
            .with_inner_size([1120.0, 820.0])
            .with_min_inner_size([860.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Activity Tracker",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

// Silence unused warnings for helpers kept for future use.
#[allow(dead_code)]
fn _unused(s: &Segment) -> (i64, i64) {
    (s.start_ms, s.end_ms)
}

#[allow(dead_code)]
fn _weekday(d: NaiveDate) -> u32 {
    d.weekday().num_days_from_monday()
}
