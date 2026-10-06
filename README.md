# Activity Tracker for Windows

A privacy-first Windows app that records **which applications you use, for how
long, and what you were doing** — then shows you a daily/weekly review of your
time on the computer.

All data stays on your machine (local SQLite database). Nothing is sent anywhere.

## Features

- **Background tracking** — samples the foreground window ~once per second:
  application (`.exe`), window title, and — for browsers — the current **URL**
  (Chrome, Edge, Firefox, Brave, … via UI Automation).
- **Idle detection** — time without keyboard/mouse input is recorded separately
  (`--idle-after`, default 3 minutes), so being away from the PC doesn't
  inflate your numbers.
- **Categories** — automatic buckets: Development, Browsing, Communication,
  Office/Docs, Media, System, Other.
- **System tray app** — pause/resume, open dashboard, exit.
- **Text reports** — `tracker report` (today) and `tracker report --week`.
- **Graphical dashboard** — weekly bars, per-day timeline of everything you
  did, application/category breakdowns, session list with titles and URLs.
- **Export** — raw segments to CSV or JSON.
- **Autostart** — start tracking automatically at login.

## Building

Requires [Rust](https://rustup.rs) (MSVC toolchain) and the MSVC C/C++
build tools (for the bundled SQLite).

```powershell
cargo build --workspace          # builds tracker.exe + tracker-dashboard.exe
cargo test  --workspace
```

Binaries land in `target\debug\` (or `target\release\` after
`cargo build --release`).

## Usage

```powershell
# Start tracking (tray icon appears; pause/exit via right-click menu)
.\target\debug\tracker.exe watch

# Start tracking headless (Ctrl+C stops)
.\target\debug\tracker.exe watch --no-tray

# Today's report / this week's report
.\target\debug\tracker.exe report
.\target\debug\tracker.exe report --week
.\target\debug\tracker.exe report 2026-10-03      # a specific day

# Graphical dashboard
.\target\debug\tracker.exe dashboard              # or run tracker-dashboard.exe

# Is it running? Where is the data?
.\target\debug\tracker.exe status

# Control a running (possibly hidden) instance — works across processes
.\target\debug\tracker.exe pause       # stop recording (keeps running, resumes on 'resume')
.\target\debug\tracker.exe resume
.\target\debug\tracker.exe stop        # graceful stop (flushes current segment)

# Export raw data (last7 days by default)
.\target\debug\tracker.exe export --from 2026-10-01 --to 2026-10-06 --format csv --out week.csv
.\target\debug\tracker.exe export --format json --out data.json

# Start at login / stop / check  (runs `watch --hidden` — no console window)
.\target\debug\tracker.exe autostart on
.\target\debug\tracker.exe autostart off
.\target\debug\tracker.exe autostart status
```

### Running like a service?

A literal Windows service cannot be used for this: services run in
"session 0", which is isolated from your interactive desktop — they cannot
see which window is in the foreground. The tracker therefore starts at
**logon** (same as Task Scheduler "At log on" / the Run registry key) inside
your session, with the console hidden (`--hidden`) and a tray icon for
control. Combine with `autostart on` + `pause`/`resume`/`stop` for
service-like operation.

### Useful flags for `watch`

| Flag | Meaning |
|---|---|
| `--interval <secs>` | Sample period (default `1.0`) |
| `--idle-after <mins>` | Minutes without input before counting as idle (default `3.0`) |
| `--no-urls` | Don't read browser URLs (titles only) — maximum privacy |
| `--no-tray` | No tray icon (headless) |
| `--hidden` | Hide the console window (used by autostart) |

## Data

- Database: `%LOCALAPPDATA%\activity-tracker\tracker.db` (SQLite, WAL mode)
- One row per *segment*: a period where the same window was in the foreground
  with the same active/idle state (`start`, `end`, `app`, `title`, `url`,
  `category`, `state`).
- Window titles and URLs can be sensitive: they are only ever written to the
  local database. Delete `tracker.db` at any time to erase everything.

## Architecture

```
crates/
  tracker-core/   sampling (Win32 + UI Automation), SQLite storage, reports
  tracker-cli/    `tracker` binary: watch / report / export / status / dashboard / autostart
  tracker-ui/     `tracker-dashboard` binary: egui weekly dashboard
```

- `watch` spawns a sampler thread + a tray event loop (tao + tray-icon).
- Browser URLs are read on a dedicated UI-Automation worker thread so a slow
  or hung provider can never stall the sampling heartbeat.
- Dev helpers: `cargo run -p tracker-core --example url_probe -- <hwnd>` and
  `… --example uia_diag -- <hwnd>` inspect UI Automation for a window.
