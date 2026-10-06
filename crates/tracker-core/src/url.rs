//! Best-effort extraction of the current page URL from browser windows.
//!
//! UI Automation calls can occasionally be slow or hang, so all UIA work runs
//! on a dedicated worker thread. The sampler polls the worker without ever
//! blocking; if the worker stops answering it is restarted (a few times) and
//! then disabled so tracking continues URL-less.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use uiautomation::patterns::UIValuePattern;
use uiautomation::types::{Handle, TreeScope, UIProperty};
use uiautomation::variants::Variant;
use uiautomation::{UIAutomation, UIElement};

const RESPONSE_DEADLINE: Duration = Duration::from_millis(8000);
const REQUERY_EVERY_TICKS: u32 = 5;
const MAX_FAILURES: u32 = 3;

#[derive(Debug)]
struct Request {
    id: u64,
    hwnd: isize,
    title: String,
}

type Response = (u64, Option<String>);

/// Non-blocking URL extractor with an internal worker thread.
pub struct UrlExtractor {
    tx: mpsc::Sender<Request>,
    rx: mpsc::Receiver<Response>,
    next_id: u64,
    in_flight: Option<(u64, (isize, String), Instant)>,
    key: (isize, String),
    url: Option<String>,
    ticks_since_query: u32,
    failures: u32,
    disabled: bool,
}

impl UrlExtractor {
    pub fn new() -> Self {
        let (tx, rx) = Self::spawn_worker();
        UrlExtractor {
            tx,
            rx,
            next_id: 1,
            in_flight: None,
            key: (0, String::new()),
            url: None,
            ticks_since_query: REQUERY_EVERY_TICKS,
            failures: 0,
            disabled: false,
        }
    }

    fn spawn_worker() -> (mpsc::Sender<Request>, mpsc::Receiver<Response>) {
        let (req_tx, req_rx) = mpsc::channel::<Request>();
        let (resp_tx, resp_rx) = mpsc::channel::<Response>();
        std::thread::spawn(move || worker_loop(req_rx, resp_tx));
        (req_tx, resp_rx)
    }

    fn restart(&mut self) {
        let (tx, rx) = Self::spawn_worker();
        self.tx = tx;
        self.rx = rx;
        self.in_flight = None;
    }

    /// Poll for a URL. Never blocks.
    ///
    /// * `hwnd` / `title` – current foreground window (call only for browsers).
    pub fn extract(&mut self, hwnd: isize, title: &str) -> Option<String> {
        if self.disabled {
            return None;
        }
        let key = (hwnd, title.to_string());

        // Collect any response that arrived since the last tick.
        while let Ok((id, url)) = self.rx.try_recv() {
            let pending = self.in_flight.take();
            match pending {
                Some((pid, pkey, _)) if pid == id => {
                    if pkey == key {
                        self.url = url;
                        self.failures = 0;
                    }
                    // Response for an outdated key: just free the slot.
                }
                other => {
                    self.in_flight = other;
                }
            }
        }

        // Detect a hung worker and restart it.
        if let Some((_, _, sent)) = self.in_flight {
            if sent.elapsed() > RESPONSE_DEADLINE {
                self.failures += 1;
                if self.failures > MAX_FAILURES {
                    self.disabled = true;
                    self.in_flight = None;
                    return None;
                }
                self.restart();
                return None;
            }
        }

        // Decide whether a fresh query is warranted.
        let key_changed = key != self.key;
        if key_changed {
            self.key = key.clone();
            self.ticks_since_query = REQUERY_EVERY_TICKS; // force a query
        } else {
            self.ticks_since_query = self.ticks_since_query.saturating_add(1);
        }

        let want_query =
            self.in_flight.is_none() && self.ticks_since_query >= REQUERY_EVERY_TICKS;
        if want_query {
            self.ticks_since_query = 0;
            let id = self.next_id;
            self.next_id += 1;
            if self
                .tx
                .send(Request {
                    id,
                    hwnd,
                    title: title.to_string(),
                })
                .is_ok()
            {
                self.in_flight = Some((id, key, Instant::now()));
            } else {
                self.failures += 1;
                if self.failures > MAX_FAILURES {
                    self.disabled = true;
                } else {
                    self.restart();
                }
            }
        }

        self.url.clone()
    }
}

fn worker_loop(rx: mpsc::Receiver<Request>, tx: mpsc::Sender<Response>) {
    let mut auto: Option<UIAutomation> = None;
    while let Ok(req) = rx.recv() {
        if auto.is_none() {
            auto = UIAutomation::new().ok();
        }
        let url = auto
            .as_mut()
            .and_then(|a| query_url(a, req.hwnd, &req.title));
        if tx.send((req.id, url)).is_err() {
            break;
        }
    }
}

/// Synchronous UIA query: find the address bar and read its value.
fn query_url(auto: &mut UIAutomation, hwnd: isize, _title: &str) -> Option<String> {
    let root = auto.element_from_handle(Handle::from(hwnd)).ok()?;

    // 1) AutomationId probes (works for Firefox and some Chromium versions).
    for id in ["URL", "urlbar-input", "url", "address_bar"] {
        let Ok(cond) =
            auto.create_property_condition(UIProperty::AutomationId, Variant::from(id), None)
        else {
            continue;
        };
        if let Ok(el) = root.find_first(TreeScope::Descendants, &cond) {
            if let Some(value) = read_value(&el) {
                return sanitize(value);
            }
        }
    }

    // 2) Chromium exposes the omnibox as an Edit control named
    //    "Address and search bar" (varies by locale/version).
    let Ok(name_cond) = auto.create_property_condition(
        UIProperty::Name,
        Variant::from("Address and search bar"),
        Some(uiautomation::types::PropertyConditionFlags::IgnoreCase),
    ) else {
        return None;
    };
    if let Ok(el) = root.find_first(TreeScope::Descendants, &name_cond) {
        if let Some(value) = read_value(&el) {
            return sanitize(value);
        }
    }

    // 3) Fall back to scanning edit controls: prefer one whose name mentions
    //    the address bar; if there is exactly one edit control, use it.
    let Ok(edit_cond) = auto.create_property_condition(
        UIProperty::ControlType,
        Variant::from(uiautomation::types::ControlType::Edit as i32),
        None,
    ) else {
        return None;
    };
    if let Ok(edits) = root.find_all(TreeScope::Descendants, &edit_cond) {
        let named = edits.iter().find(|el| {
            el.get_name()
                .map(|n| n.to_lowercase().contains("address"))
                .unwrap_or(false)
        });
        let chosen = named.or_else(|| if edits.len() == 1 { edits.first() } else { None });
        if let Some(el) = chosen {
            if let Some(value) = read_value(el) {
                return sanitize(value);
            }
        }
    }

    // 4) Last resort: the focused element, if it is an edit box.
    if let Ok(focused) = auto.get_focused_element() {
        if let Ok(ct) = focused.get_control_type() {
            if ct == uiautomation::types::ControlType::Edit {
                if let Some(value) = read_value(&focused) {
                    return sanitize(value);
                }
            }
        }
    }
    None
}

fn read_value(el: &UIElement) -> Option<String> {
    if let Ok(pattern) = el.get_pattern::<UIValuePattern>() {
        if let Ok(v) = pattern.get_value() {
            return Some(v);
        }
    }
    // Some providers only expose the value as the element name.
    if let Ok(name) = el.get_name() {
        if looks_like_url(&name) {
            return Some(name);
        }
    }
    None
}

fn looks_like_url(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() || s.len() > 2048 || s.contains('\n') {
        return false;
    }
    s.contains("://") || s.starts_with("about:") || s.starts_with("chrome://")
        || s.starts_with("edge://")
}

fn sanitize(value: String) -> Option<String> {
    let v = value.trim().replace(['\r', '\n'], " ");
    if v.is_empty() || v.len() > 2048 {
        None
    } else {
        Some(v)
    }
}
