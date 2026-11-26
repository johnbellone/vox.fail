//! Where the app keeps your files, and its log.
//!
//! Your settings, questions, clips, gags and sounds live in one folder:
//! - next to the program when that folder is writable (a portable copy, or running from source);
//! - otherwise (an installed copy in Program Files) in `%APPDATA%\Block Avatar` on Windows,
//!   `~/.local/share/block-avatar` elsewhere. `BLOCK_AVATAR_HOME` overrides both.
//!
//! Everything the app reports goes to `avatar.log` there (the previous run's is kept as
//! `avatar.prev.log`), so the windowless Windows build still has a record; the latest problems
//! are also shown on the control page.

use std::{
    collections::VecDeque,
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

fn writable(dir: &std::path::Path) -> bool {
    let probe = dir.join(".block-avatar-write-test");
    let ok = std::fs::write(&probe, b"ok").is_ok();
    let _ = std::fs::remove_file(probe);
    ok
}

/// The folder for your settings and everything the app saves.
pub fn user_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        if let Some(h) = std::env::var_os("BLOCK_AVATAR_HOME") {
            let p = PathBuf::from(h);
            let _ = std::fs::create_dir_all(&p);
            return p;
        }
        let base = crate::asset_base();
        if writable(&base) {
            return base;
        }
        let p = if cfg!(windows) {
            std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Block Avatar"))
        } else {
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
                .map(|d| d.join("block-avatar"))
        }
        .unwrap_or(base);
        let _ = std::fs::create_dir_all(&p);
        p
    })
    .clone()
}

struct Log {
    file: Option<std::fs::File>,
    problems: VecDeque<String>,
}

fn log() -> &'static Mutex<Log> {
    static L: OnceLock<Mutex<Log>> = OnceLock::new();
    L.get_or_init(|| {
        let dir = user_dir();
        let path = dir.join("avatar.log");
        let _ = std::fs::rename(&path, dir.join("avatar.prev.log"));
        Mutex::new(Log {
            file: std::fs::File::create(path).ok(),
            problems: VecDeque::new(),
        })
    })
}

fn stamp() -> String {
    let s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        % 86400;
    format!("{:02}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
}

pub fn write(problem: bool, msg: &str) {
    if problem {
        eprintln!("{msg}");
    } else {
        println!("{msg}");
    }
    if let Ok(mut l) = log().lock() {
        let line = format!(
            "{} {}{msg}\n",
            stamp(),
            if problem { "PROBLEM " } else { "" }
        );
        if let Some(f) = l.file.as_mut() {
            let _ = f.write_all(line.as_bytes());
        }
        if problem {
            l.problems.push_back(format!("{} {msg}", stamp()));
            while l.problems.len() > 12 {
                l.problems.pop_front();
            }
        }
    }
}

/// The latest problems, newest last, for the control page.
pub fn problems() -> Vec<String> {
    log()
        .lock()
        .map(|l| l.problems.iter().cloned().collect())
        .unwrap_or_default()
}

pub fn clear_problems() {
    if let Ok(mut l) = log().lock() {
        l.problems.clear();
    }
}

/// Something worth knowing (also in avatar.log).
#[macro_export]
macro_rules! note {
    ($($t:tt)*) => { $crate::applog::write(false, &format!($($t)*)) };
}

/// Something went wrong that you should see: log file and the control page's problem list.
#[macro_export]
macro_rules! problem {
    ($($t:tt)*) => { $crate::applog::write(true, &format!($($t)*)) };
}

/// Opens a web page in your browser.
pub fn open_url(url: &str) {
    let r = if cfg!(windows) {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if let Err(e) = r {
        note!("Open {url} in your browser ({e})");
    }
}

/// Opens a folder in Explorer / Finder / the file manager.
pub fn open_folder(p: &std::path::Path) {
    let _ = std::fs::create_dir_all(p);
    let prog = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(prog).arg(p).spawn() {
        problem!("Couldn't open {}: {e}", p.display());
    }
}

/// The Windows build has no console window; when it's started from a terminal with a
/// command-line option (`--list-mics`, `--analyze`), borrow the terminal's console so the output
/// shows up there.
pub fn attach_console_for_cli() {
    #[cfg(any(windows, winchk))]
    {
        let cli = std::env::args().any(|a| a == "--list-mics" || a == "--analyze" || a == "--help");
        if cli {
            // SAFETY: plain Win32 call with a constant argument
            unsafe {
                windows_sys::Win32::System::Console::AttachConsole(
                    windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
                );
            }
        }
    }
}
