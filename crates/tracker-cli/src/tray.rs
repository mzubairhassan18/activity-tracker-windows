//! System-tray icon + menu, running on a tao event loop.
//!
//! Menu actions:
//!   * Open dashboard – launches the dashboard window
//!   * Pause tracking  – toggles sampling (persisted to the database)
//!   * Exit tracker    – flushes and quits

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder, TrayIconEvent};

use crate::launch_dashboard;

#[derive(Debug, Clone, Copy)]
enum UserEvent {
    Wake,
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
    let paused = opts.paused.clone();

    let menu = Menu::new();
    let open_item = MenuItem::new("Open dashboard", true, None);
    let pause_item =
        CheckMenuItem::new("Pause tracking", true, paused.load(Ordering::Relaxed), None);
    let exit_item = MenuItem::new("Exit tracker", true, None);
    menu.append(&open_item)?;
    menu.append(&pause_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&exit_item)?;

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // Wake the event loop when the sampler thread exits on its own
    // (`tracker stop`, sampler error), so the tray does not linger.
    {
        let watchdog_proxy = event_loop.create_proxy();
        let done_flag = opts.sampler_done.clone();
        std::thread::spawn(move || loop {
            if done_flag.load(Ordering::Relaxed) {
                let _ = watchdog_proxy.send_event(UserEvent::Wake);
                break;
            }
            std::thread::sleep(Duration::from_millis(400));
        });
    }

    // Menu clicks and tray events arrive on the message-pump thread; wake the
    // event loop so the main closure can process them.
    let proxy_for_menu = proxy.clone();
    MenuEvent::set_event_handler(Some(move |_event| {
        let _ = proxy_for_menu.send_event(UserEvent::Wake);
    }));
    TrayIconEvent::set_event_handler(Some(move |_event| {
        let _ = proxy.send_event(UserEvent::Wake);
    }));

    let tray = TrayIconBuilder::new()
        .with_tooltip("Activity Tracker — click for menu")
        .with_menu(Box::new(menu))
        .with_icon(make_icon())
        .build()?;

    let opts = Arc::new(opts);
    let opts_exit = opts.clone();
    let pause_item_exit = pause_item.clone();

    event_loop.run(move |event, _window_target, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::UserEvent(UserEvent::Wake) => {
                // Sampler exited by itself (stop command / error): shut the
                // tray down as well (nothing left to flush).
                if opts.sampler_done.load(Ordering::Relaxed) {
                    *control_flow = ControlFlow::Exit;
                    return;
                }
                while let Ok(menu_event) = MenuEvent::receiver().try_recv() {
                    if menu_event.id() == open_item.id() {
                        if let Err(e) = launch_dashboard() {
                            eprintln!("failed to open dashboard: {e:?}");
                        }
                    } else if menu_event.id() == pause_item.id() {
                        let new_state = !opts.paused.load(Ordering::Relaxed);
                        opts.paused.store(new_state, Ordering::Relaxed);
                        pause_item_exit.set_checked(new_state);
                        (opts.on_pause_change)(new_state);
                    } else if menu_event.id() == exit_item.id() {
                        shutdown(&opts_exit, control_flow);
                    }
                }
                while let Ok(event) = TrayIconEvent::receiver().try_recv() {
                    if let TrayIconEvent::DoubleClick { .. } = event {
                        if let Err(e) = launch_dashboard() {
                            eprintln!("failed to open dashboard: {e:?}");
                        }
                    }
                }
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
