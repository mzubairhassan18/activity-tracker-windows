//! Classification of executables into categories and friendly display names.

use crate::model::Category;

/// Well-known executable -> friendly display name.
fn friendly(app: &str) -> Option<&'static str> {
    Some(match app {
        "chrome.exe" => "Google Chrome",
        "msedge.exe" => "Microsoft Edge",
        "firefox.exe" => "Mozilla Firefox",
        "brave.exe" => "Brave",
        "opera.exe" => "Opera",
        "vivaldi.exe" => "Vivaldi",
        "code.exe" => "Visual Studio Code",
        "code - insiders.exe" => "VS Code Insiders",
        "devenv.exe" => "Visual Studio",
        "rider64.exe" => "JetBrains Rider",
        "idea64.exe" => "IntelliJ IDEA",
        "pycharm64.exe" => "PyCharm",
        "webstorm64.exe" => "WebStorm",
        "clion64.exe" => "CLion",
        "goland64.exe" => "GoLand",
        "rustrover64.exe" => "RustRover",
        "sublime_text.exe" => "Sublime Text",
        "notepad++.exe" => "Notepad++",
        "atom.exe" => "Atom",
        "cursor.exe" => "Cursor",
        "windsurf.exe" => "Windsurf",
        "rust-analyzer.exe" => "rust-analyzer",
        "slack.exe" => "Slack",
        "discord.exe" => "Discord",
        "teams.exe" => "Microsoft Teams",
        "ms-teams.exe" => "Microsoft Teams",
        "telegram.exe" => "Telegram",
        "whatsapp.exe" => "WhatsApp",
        "zoom.exe" => "Zoom",
        "outlook.exe" => "Outlook",
        "winword.exe" => "Microsoft Word",
        "excel.exe" => "Microsoft Excel",
        "powerpnt.exe" => "Microsoft PowerPoint",
        "onenote.exe" => "OneNote",
        "obsidian.exe" => "Obsidian",
        "notion.exe" => "Notion",
        "spotify.exe" => "Spotify",
        "vlc.exe" => "VLC media player",
        "vlc64.exe" => "VLC media player",
        "steam.exe" => "Steam",
        "epicgameslauncher.exe" => "Epic Games Launcher",
        "explorer.exe" => "File Explorer",
        "windows terminal.exe" => "Windows Terminal",
        "windowsterminal.exe" => "Windows Terminal",
        "wt.exe" => "Windows Terminal",
        "cmd.exe" => "Command Prompt",
        "powershell.exe" => "PowerShell",
        "pwsh.exe" => "PowerShell",
        "taskmgr.exe" => "Task Manager",
        "systemsettings.exe" => "Windows Settings",
        "applicationframehost.exe" => "UWP app",
        "searchhost.exe" => "Windows Search",
        "startmenuexperiencehost.exe" => "Start Menu",
        "lockapp.exe" => "Lock screen",
        "widgets.exe" => "Widgets",
        "yourphone.exe" => "Phone Link",
        "snippingtool.exe" => "Snipping Tool",
        "paint.exe" => "Paint",
        "photos.exe" => "Photos",
        _ => return None,
    })
}

/// Human friendly name for an executable.
pub fn friendly_name(app: &str) -> String {
    friendly(app).map(str::to_string).unwrap_or_else(|| {
        let stem = app.strip_suffix(".exe").unwrap_or(app);
        if stem.is_empty() {
            "Unknown".to_string()
        } else {
            // Keep original casing for unknown programs when possible.
            let mut c = stem.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => stem.to_string(),
            }
        }
    })
}

const DEVELOPMENT: &[&str] = &[
    "code.exe",
    "code - insiders.exe",
    "cursor.exe",
    "windsurf.exe",
    "devenv.exe",
    "rider64.exe",
    "idea64.exe",
    "pycharm64.exe",
    "webstorm64.exe",
    "clion64.exe",
    "goland64.exe",
    "rustrover64.exe",
    "sublime_text.exe",
    "notepad++.exe",
    "atom.exe",
    "rust-analyzer.exe",
    "cargo.exe",
    "rustc.exe",
    "node.exe",
    "git-bash.exe",
    "putty.exe",
    "winscp.exe",
    "filezilla.exe",
    "postman.exe",
    "insomnia.exe",
    "docker desktop.exe",
    "wslrelay.exe",
    "opencode.exe",
    "codex.exe",
    "claude.exe",
    "aider.exe",
    "cline.exe",
    "continue.exe",
];

const BROWSING: &[&str] = &[
    "chrome.exe",
    "msedge.exe",
    "firefox.exe",
    "brave.exe",
    "opera.exe",
    "vivaldi.exe",
    "browser.exe",
    "maxthon.exe",
    "waterfox.exe",
    "floorp.exe",
];

const COMMUNICATION: &[&str] = &[
    "slack.exe",
    "discord.exe",
    "teams.exe",
    "ms-teams.exe",
    "telegram.exe",
    "whatsapp.exe",
    "zoom.exe",
    "outlook.exe",
    "thunderbird.exe",
    "element.exe",
    "signal.exe",
    "skype.exe",
    "yourphone.exe",
    "phone experience host.exe",
];

const OFFICE: &[&str] = &[
    "winword.exe",
    "excel.exe",
    "powerpnt.exe",
    "onenote.exe",
    "acrord32.exe",
    "sumatrapdf.exe",
    "foxitreader.exe",
    "obsidian.exe",
    "notion.exe",
    "evernote.exe",
    "soffice.exe",
    "wordpad.exe",
    "write.exe",
];

const MEDIA: &[&str] = &[
    "spotify.exe",
    "vlc.exe",
    "vlc64.exe",
    "mpv.exe",
    "foobar2000.exe",
    "musicbee.exe",
    "steam.exe",
    "epicgameslauncher.exe",
    "battle.net.exe",
    "itunes.exe",
    "netflix.exe",
    "plex.exe",
    "jellyfin.exe",
    "photos.exe",
];

const SYSTEM: &[&str] = &[
    "explorer.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "windowsterminal.exe",
    "windows terminal.exe",
    "wt.exe",
    "taskmgr.exe",
    "systemsettings.exe",
    "control.exe",
    "applicationframehost.exe",
    "searchhost.exe",
    "searchapp.exe",
    "startmenuexperiencehost.exe",
    "lockapp.exe",
    "sihost.exe",
    "shellexperiencehost.exe",
    "ctfmon.exe",
    "dwm.exe",
    "taskhostw.exe",
    "widgets.exe",
    "widgetservice.exe",
    "securityhealthsystray.exe",
];

fn in_list(list: &[&str], app: &str) -> bool {
    list.iter().any(|x| *x == app)
}

/// Map an executable name (lower case, with .exe) to a category.
pub fn categorize(app: &str) -> Category {
    if in_list(DEVELOPMENT, app) {
        Category::Development
    } else if in_list(BROWSING, app) {
        Category::Browsing
    } else if in_list(COMMUNICATION, app) {
        Category::Communication
    } else if in_list(OFFICE, app) {
        Category::Office
    } else if in_list(MEDIA, app) {
        Category::Media
    } else if in_list(SYSTEM, app) {
        Category::System
    } else {
        Category::Other
    }
}

/// Executables whose foreground window we try to read a URL from.
pub fn is_browser(app: &str) -> bool {
    in_list(BROWSING, app)
}

/// Strip noisy browser suffixes from window titles ("… - Google Chrome").
pub fn clean_title(app: &str, title: &str) -> String {
    let t = title.trim();
    let suffixes: &[&str] = if is_browser(app) {
        &[
            " - Google Chrome",
            " — Google Chrome",
            " - Microsoft Edge",
            " — Microsoft Edge",
            " - Mozilla Firefox",
            " — Mozilla Firefox",
            " - Brave",
            " — Brave",
            " - Opera",
            " — Opera",
        ]
    } else {
        &[]
    };
    for s in suffixes {
        if t.len() > s.len() && t.ends_with(s) {
            return t[..t.len() - s.len()].trim().to_string();
        }
    }
    t.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories() {
        assert_eq!(categorize("code.exe"), Category::Development);
        assert_eq!(categorize("chrome.exe"), Category::Browsing);
        assert_eq!(categorize("slack.exe"), Category::Communication);
        assert_eq!(categorize("excel.exe"), Category::Office);
        assert_eq!(categorize("spotify.exe"), Category::Media);
        assert_eq!(categorize("explorer.exe"), Category::System);
        assert_eq!(categorize("random-thing.exe"), Category::Other);
    }

    #[test]
    fn friendly_names() {
        assert_eq!(friendly_name("code.exe"), "Visual Studio Code");
        assert_eq!(friendly_name("foo.exe"), "Foo");
    }

    #[test]
    fn titles() {
        assert_eq!(
            clean_title("chrome.exe", "GitHub - Google Chrome"),
            "GitHub"
        );
        assert_eq!(clean_title("code.exe", "main.rs"), "main.rs");
    }
}
