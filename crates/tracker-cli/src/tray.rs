//! System-tray icon + menu, running on a tao event loop.
//!
//! Menu actions:
//!   * Open dashboard – launches the dashboard window
//!   * Pause tracking  – toggles sampling (persisted to the database)
//!   * Exit tracker    – flushes and quits
//!
//! IMPORTANT: both muda and tray-icon route an event to `set_event_handler`
//! *instead of* the `receiver()` channel — see `MenuEvent::send` and
//! `TrayIconEvent::send`. Installing a handler therefore *disables* the
//! channel, so `MenuEvent::receiver().try_recv()` can never yield anything.
//! Events must be forwarded through the proxy as the event payload and handled
//! on the loop thread, where the non-`Send` menu items are still reachable.
//! See https://github.com/tauri-apps/tray-icon/issues/209.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder, TrayIconEvent};

use crate::launch_dashboard;
use crate::log_line;

#[derive(Debug, Clone)]
enum UserEvent {
    /// A menu item was activated.
    Menu(MenuEvent),
    /// A tray icon click (left / right / double-click).
    Tray(TrayIconEvent),
    /// The sampler thread exited on its own: shut the tray down as well.
    SamplerDone,
}

pub struct TrayOptions {
    pub stop: Arc<AtomicBool>,
    pub paused: Arc<AtomicBool>,
    pub interval_ms: u64,
    /// Set by the sampler thread when it exits (e.g. after `tracker stop`).
    pub sampler_done: Arc<AtomicBool>,
    /// Called with the new paused state whenever the menu toggles it.
    pub on_pause_change: Box<dyn Fn(bool)>,
}

pub fn run(opts: TrayOptions) -> Result<()> {
    let opts = Arc::new(opts);

    let menu = Menu::new();
    let open_item = MenuItem::new("Open dashboard", true, None);
    let pause_item =
        CheckMenuItem::new("Pause tracking", true, opts.paused.load(Ordering::Relaxed), None);
    let exit_item = MenuItem::new("Exit tracker", true, None);
    menu.append(&open_item)?;
    menu.append(&pause_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&exit_item)?;

    // Owned copies of the ids: the menu items are `Rc`-based and cannot cross
    // into the `Send` handler closures, but their ids are plain strings.
    let open_id = open_item.id().0.clone();
    let pause_id = pause_item.id().0.clone();
    let exit_id = exit_item.id().0.clone();

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // Wake the event loop when the sampler thread exits on its own
    // (`tracker stop`, sampler error), so the tray does not linger.
    {
        let watchdog_proxy = event_loop.create_proxy();
        let done_flag = opts.sampler_done.clone();
        std::thread::spawn(move || loop {
            if done_flag.load(Ordering::Relaxed) {
                let _ = watchdog_proxy.send_event(UserEvent::SamplerDone);
                break;
            }
            std::thread::sleep(Duration::from_millis(400));
        });
    }

    // Forward the events themselves so the loop is woken *and* has the payload.
    {
        let menu_proxy = proxy.clone();
        MenuEvent::set_event_handler(Some(move |event| {
            let _ = menu_proxy.send_event(UserEvent::Menu(event));
        }));
        TrayIconEvent::set_event_handler(Some(move |event| {
            let _ = proxy.send_event(UserEvent::Tray(event));
        }));
    }

    let tray = TrayIconBuilder::new()
        .with_tooltip("Activity Tracker — click for menu")
        .with_menu(Box::new(menu))
        .with_icon(make_icon())
        .build()?;

    event_loop.run(move |event, _window_target, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::UserEvent(UserEvent::Menu(menu_event)) => {
                let id = &menu_event.id().0;
                if *id == open_id {
                    if let Err(e) = launch_dashboard() {
                        // stderr is invisible under `--hidden`, so log to disk.
                        log_line(&format!("open dashboard failed: {e:#}"));
                    }
                } else if *id == pause_id {
                    let new_state = !opts.paused.load(Ordering::Relaxed);
                    opts.paused.store(new_state, Ordering::Relaxed);
                    pause_item.set_checked(new_state);
                    (opts.on_pause_change)(new_state);
                } else if *id == exit_id {
                    shutdown(&opts, control_flow);
                }
            }
            Event::UserEvent(UserEvent::Tray(tray_event)) => {
                if let TrayIconEvent::DoubleClick { .. } = tray_event {
                    if let Err(e) = launch_dashboard() {
                        log_line(&format!("open dashboard failed: {e:#}"));
                    }
                }
            }
            Event::UserEvent(UserEvent::SamplerDone) => {
                // Nothing left to flush.
                *control_flow = ControlFlow::Exit;
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => shutdown(&opts, control_flow),
            _ => {}
        }
        let _ = &tray; // keep the tray icon alive for the loop's lifetime
    })
}

fn shutdown(opts: &TrayOptions, control_flow: &mut ControlFlow) {
    opts.stop.store(true, Ordering::Relaxed);
    // Give the sampler enough time to flush its open segment.
    let wait = (opts.interval_ms * 3 + 700).min(4000);
    std::thread::sleep(Duration::from_millis(wait));
    *control_flow = ControlFlow::Exit;
}

/// A simple generated icon (dark rounded square with an accent "clock" dot).
fn make_icon() -> Icon {
    const SIZE: usize = 32;
    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    let center = (SIZE as f32) / 2.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let i = (y * SIZE + x) * 4;
            let dx = x as f32 + 0.5 - center;
            let dy = y as f32 + 0.5 - center;
            let d = (dx * dx + dy * dy).sqrt();
            // Rounded background.
            let corner = {
                let cx = (x as f32 - center).abs().max(0.0);
                let cy = (y as f32 - center).abs().max(0.0);
                let rr = (cx.powi(2) + cy.powi(2)).sqrt();
                rr < 13.0
            };
            if corner {
                if d < 10.5 {
                    // Face
                    rgba[i] = 240;
                    rgba[i + 1] = 244;
                    rgba[i + 2] = 248;
                    rgba[i + 3] = 255;
                } else {
                    // Ring
                    rgba[i] = 47;
                    rgba[i + 1] = 128;
                    rgba[i + 2] = 237;
                    rgba[i + 3] = 255;
                }
            }
        }
    }
    // Minute hand.
    for t in 0..8 {
        let x = (center as usize) + 0;
        let y = (center as usize).saturating_sub(t + 1);
        let i = (y * SIZE + x) * 4;
        rgba[i] = 30;
        rgba[i + 1] = 41;
        rgba[i + 2] = 59;
        rgba[i + 3] = 255;
    }
    Icon::from_rgba(rgba, SIZE as u32, SIZE as u32).expect("valid icon")
}
