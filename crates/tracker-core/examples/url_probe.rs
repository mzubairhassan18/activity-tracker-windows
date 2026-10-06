//! Dev helper: probe the browser-URL extractor for a given window handle.
//!
//! Usage: cargo run -p tracker-core --example url_probe -- <hwnd>
//! Get the hwnd from e.g. PowerShell: (Get-Process chrome | ? MainWindowHandle -ne 0).MainWindowHandle

use std::time::Duration;

use tracker_core::url::UrlExtractor;

fn main() {
    let hwnd: isize = std::env::args()
        .nth(1)
        .expect("usage: url_probe <hwnd>")
        .parse()
        .expect("hwnd must be an integer (decimal or 0x…)");
    println!("probing hwnd {hwnd:#x} …");
    let mut ext = UrlExtractor::new();
    for _ in 0..60 {
        if let Some(url) = ext.extract(hwnd, "probe") {
            println!("URL: {url}");
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    eprintln!("no URL within6 seconds");
    std::process::exit(1);
}
