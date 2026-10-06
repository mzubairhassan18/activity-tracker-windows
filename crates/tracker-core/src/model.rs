use serde::{Deserialize, Serialize};

/// Whether the user was interacting with the PC during a segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivityState {
    Active,
    Idle,
}

impl ActivityState {
    pub fn as_str(self) -> &'static str {
        match self {
            ActivityState::Active => "active",
            ActivityState::Idle => "idle",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "active" => Some(ActivityState::Active),
            "idle" => Some(ActivityState::Idle),
            _ => None,
        }
    }
}

/// Coarse activity category derived from the executable name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Category {
    Development,
    Browsing,
    Communication,
    Office,
    Media,
    System,
    Other,
}

impl Category {
    pub const ALL: [Category; 7] = [
        Category::Development,
        Category::Browsing,
        Category::Communication,
        Category::Office,
        Category::Media,
        Category::System,
        Category::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Category::Development => "development",
            Category::Browsing => "browsing",
            Category::Communication => "communication",
            Category::Office => "office",
            Category::Media => "media",
            Category::System => "system",
            Category::Other => "other",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Category::Development => "Development",
            Category::Browsing => "Browsing",
            Category::Communication => "Communication",
            Category::Office => "Office / Docs",
            Category::Media => "Media",
            Category::System => "System",
            Category::Other => "Other",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "development" => Some(Category::Development),
            "browsing" => Some(Category::Browsing),
            "communication" => Some(Category::Communication),
            "office" => Some(Category::Office),
            "media" => Some(Category::Media),
            "system" => Some(Category::System),
            "other" => Some(Category::Other),
            _ => None,
        }
    }
}

/// A continuous period during which the same window was in the foreground
/// with the same activity state (active or idle).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    /// Database id (0 for unsaved segments).
    pub id: i64,
    /// Unix epoch milliseconds.
    pub start_ms: i64,
    /// Unix epoch milliseconds (exclusive end).
    pub end_ms: i64,
    /// Executable file name, lower case, e.g. "chrome.exe".
    pub app: String,
    /// Foreground window title (may contain sensitive information).
    pub title: String,
    /// Page URL for browsers; empty otherwise.
    pub url: String,
    pub category: Category,
    pub state: ActivityState,
}

impl Segment {
    pub fn duration_ms(&self) -> i64 {
        (self.end_ms - self.start_ms).max(0)
    }

    pub fn duration_secs(&self) -> u64 {
        (self.duration_ms() / 1000).max(0) as u64
    }

    /// Milliseconds of overlap between this segment and [rs, re).
    pub fn overlap_ms(&self, rs: i64, re: i64) -> i64 {
        let s = self.start_ms.max(rs);
        let e = self.end_ms.min(re);
        (e - s).max(0)
    }
}
