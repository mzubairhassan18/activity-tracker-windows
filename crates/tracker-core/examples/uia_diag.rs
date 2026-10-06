//! Dev helper: step-by-step UIA diagnostics for a window handle.

use uiautomation::types::{ControlType, Handle, TreeScope, UIProperty};
use uiautomation::variants::Variant;
use uiautomation::{UIAutomation, UIElement};

fn main() {
    let hwnd: isize = std::env::args()
        .nth(1)
        .expect("usage: uia_diag <hwnd>")
        .parse()
        .expect("integer hwnd");

    let auto = match UIAutomation::new() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("UIAutomation::new failed: {e:?}");
            return;
        }
    };
    println!("COM/UIA initialized");

    let root = match auto.element_from_handle(Handle::from(hwnd)) {
        Ok(el) => el,
        Err(e) => {
            eprintln!("element_from_handle failed: {e:?}");
            return;
        }
    };
    println!(
        "root: name={:?} ct={:?} aid={:?}",
        root.get_name().ok(),
        root.get_control_type().ok(),
        root.get_automation_id().ok()
    );

    for id in ["URL", "urlbar-input", "url", "address_bar", "Address Bar"] {
        match auto.create_property_condition(UIProperty::AutomationId, Variant::from(id), None) {
            Ok(cond) => match root.find_first(TreeScope::Descendants, &cond) {
                Ok(el) => {
                    println!(
                        "found aid={id:?}: name={:?} ct={:?}",
                        el.get_name().ok(),
                        el.get_control_type().ok()
                    );
                    match el.get_pattern::<uiautomation::patterns::UIValuePattern>() {
                        Ok(p) => println!("  value pattern: {:?}", p.get_value()),
                        Err(e) => println!("  value pattern failed: {e:?}"),
                    }
                }
                Err(e) => println!("aid={id:?}: find_first error {e:?}"),
            },
            Err(e) => println!("aid={id:?}: condition error {e:?}"),
        }
    }

    // List some edit controls.
    if let Ok(cond) = auto.create_property_condition(
        UIProperty::ControlType,
        Variant::from(ControlType::Edit as i32),
        None,
    ) {
        match root.find_all(TreeScope::Descendants, &cond) {
            Ok(els) => {
                println!("edit controls: {}", els.len());
                for (i, el) in els.iter().take(8).enumerate() {
                    let _ = i;
                    print_el(&el);
                }
            }
            Err(e) => println!("find_all edits failed: {e:?}"),
        }
    }

    // Focused element.
    match auto.get_focused_element() {
        Ok(el) => {
            println!("focused:");
            print_el(&el);
        }
        Err(e) => println!("get_focused_element failed: {e:?}"),
    }
}

fn print_el(el: &UIElement) {
    let mut line = format!(
        "  ct={:?} aid={:?} name={:?}",
        el.get_control_type().ok(),
        el.get_automation_id().ok(),
        el.get_name().ok()
    );
    if let Ok(p) = el.get_pattern::<uiautomation::patterns::UIValuePattern>() {
        if let Ok(v) = p.get_value() {
            line.push_str(&format!(" value={v:?}"));
        }
    }
    println!("{line}");
}
