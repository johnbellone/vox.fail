//! Global hotkeys: key combinations that work while a game (or anything else) has focus, each
//! sending one control-page command. Windows only; written in `hotkeys.txt` in your files folder
//! and editable on the control page's Hotkeys tab:
//!
//!     Ctrl+Alt+T = throw:tomato
//!     Ctrl+Shift+F5 = face:3
//!
//! Modifiers: Ctrl, Alt, Shift, Win. Keys: A-Z, 0-9, F1-F24, Num0-Num9, Space, Enter, Tab,
//! Left, Right, Up, Down, Home, End, PageUp, PageDown, Insert, Delete, and - = [ ] ; ' , . / \ `.

use std::sync::{
    Arc, Mutex,
    mpsc::{self, Receiver},
};

use bevy::prelude::*;

pub const MOD_ALT: u32 = 1;
pub const MOD_CTRL: u32 = 2;
pub const MOD_SHIFT: u32 = 4;
pub const MOD_WIN: u32 = 8;

const DEFAULTS: &str = "# Global hotkeys: they work while a game has focus.\n# combination = control-page command   (see README: Control it while you're in a game)\nCtrl+Alt+1 = face:0\nCtrl+Alt+2 = face:1\nCtrl+Alt+3 = face:2\nCtrl+Alt+4 = face:4\nCtrl+Alt+T = throw:tomato\nCtrl+Alt+C = throw:confetti\nCtrl+Alt+H = emote:hop\nCtrl+Alt+S = emote:spin\nCtrl+Alt+M = mute\nCtrl+Alt+E = clip\nCtrl+Alt+N = ask:next\nCtrl+Alt+D = ask:done\nCtrl+Alt+Right = model:next\n";

#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    /// as written, tidied ("Ctrl+Alt+T")
    pub keys: String,
    pub mods: u32,
    pub vk: u32,
    pub cmd: String,
}

/// Windows virtual-key code for a key name.
pub fn vk_of(name: &str) -> Option<u32> {
    let n = name.trim();
    let up = n.to_ascii_uppercase();
    if up.len() == 1 {
        let c = up.as_bytes()[0];
        if c.is_ascii_uppercase() || c.is_ascii_digit() {
            return Some(c as u32);
        }
        return Some(match c {
            b'-' => 0xBD,
            b'=' => 0xBB,
            b'[' => 0xDB,
            b']' => 0xDD,
            b';' => 0xBA,
            b'\'' => 0xDE,
            b',' => 0xBC,
            b'.' => 0xBE,
            b'/' => 0xBF,
            b'\\' => 0xDC,
            b'`' => 0xC0,
            _ => return None,
        });
    }
    if let Some(f) = up
        .strip_prefix('F')
        .and_then(|d| d.parse::<u32>().ok())
        .filter(|f| (1..=24).contains(f))
    {
        return Some(0x70 + f - 1);
    }
    if let Some(d) = up
        .strip_prefix("NUM")
        .and_then(|d| d.parse::<u32>().ok())
        .filter(|d| *d <= 9)
    {
        return Some(0x60 + d);
    }
    Some(match up.as_str() {
        "SPACE" => 0x20,
        "ENTER" | "RETURN" => 0x0D,
        "TAB" => 0x09,
        "LEFT" => 0x25,
        "UP" => 0x26,
        "RIGHT" => 0x27,
        "DOWN" => 0x28,
        "HOME" => 0x24,
        "END" => 0x23,
        "PAGEUP" => 0x21,
        "PAGEDOWN" => 0x22,
        "INSERT" => 0x2D,
        "DELETE" => 0x2E,
        "PAUSE" => 0x13,
        _ => return None,
    })
}

/// "ctrl + alt + t" -> (Ctrl+Alt+T, mods, vk). Needs at least one modifier unless it's an F key
/// (a plain letter would stop you typing it anywhere).
pub fn parse_combo(s: &str) -> Option<(String, u32, u32)> {
    let parts: Vec<&str> = s
        .split('+')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    let (key, mods_s) = parts.split_last()?;
    let mut mods = 0;
    for m in mods_s {
        mods |= match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => MOD_CTRL,
            "alt" => MOD_ALT,
            "shift" => MOD_SHIFT,
            "win" | "windows" | "super" | "meta" => MOD_WIN,
            _ => return None,
        };
    }
    let vk = vk_of(key)?;
    let is_f = (0x70..=0x87).contains(&vk) || vk == 0x13;
    if mods == 0 && !is_f {
        return None;
    }
    let mut name = String::new();
    for (bit, label) in [
        (MOD_CTRL, "Ctrl"),
        (MOD_ALT, "Alt"),
        (MOD_SHIFT, "Shift"),
        (MOD_WIN, "Win"),
    ] {
        if mods & bit != 0 {
            name += label;
            name += "+";
        }
    }
    let k = key.trim();
    name += &if k.len() == 1 {
        k.to_ascii_uppercase()
    } else {
        let mut c = k.chars();
        c.next()
            .map(|f| f.to_ascii_uppercase().to_string() + &c.as_str().to_ascii_lowercase())
            .unwrap_or_default()
            .replace("Pageup", "PageUp")
            .replace("Pagedown", "PageDown")
    };
    Some((name, mods, vk))
}

/// Reads hotkeys.txt; lines that don't parse come back as errors.
pub fn parse(text: &str) -> (Vec<Binding>, Vec<String>) {
    let mut out: Vec<Binding> = vec![];
    let mut bad = vec![];
    for l in text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let Some((k, c)) = l.split_once('=') else {
            bad.push(format!("no '=' in \"{l}\""));
            continue;
        };
        // "Ctrl+=" style lines: the key itself is '='
        let (k, c) = match c.trim_start().strip_prefix('=') {
            Some(rest) if k.trim().ends_with('+') => (format!("{}=", k.trim()), rest.to_string()),
            _ => (k.to_string(), c.to_string()),
        };
        let cmd = c.trim().to_string();
        match parse_combo(&k) {
            Some((keys, mods, vk)) if !cmd.is_empty() => {
                out.retain(|b| !(b.mods == mods && b.vk == vk));
                out.push(Binding {
                    keys,
                    mods,
                    vk,
                    cmd,
                });
            }
            _ => bad.push(format!("can't use \"{}\"", k.trim())),
        }
    }
    (out, bad)
}

pub fn write(bindings: &[Binding]) -> String {
    let mut s = String::from(
        "# Global hotkeys: they work while a game has focus.\n# combination = control-page command   (see README: Control it while you're in a game)\n",
    );
    for b in bindings {
        s += &format!("{} = {}\n", b.keys, b.cmd);
    }
    s
}

fn path() -> std::path::PathBuf {
    crate::applog::user_dir().join("hotkeys.txt")
}

#[derive(Resource)]
pub struct Hotkeys {
    pub bindings: Vec<Binding>,
    /// per binding: registered with Windows, or why not
    pub state: Arc<Mutex<Vec<(String, Result<(), String>)>>>,
    pub problems: Vec<String>,
    rx: Mutex<Receiver<String>>,
    #[allow(dead_code)]
    tx: mpsc::Sender<String>,
    /// tells the Windows hotkey thread to re-read the bindings
    #[allow(dead_code)]
    thread: Option<u32>,
    shared: Arc<Mutex<Vec<Binding>>>,
}

impl Hotkeys {
    pub fn start() -> Self {
        let p = path();
        if !p.exists() {
            let _ = std::fs::write(&p, DEFAULTS);
        }
        let (bindings, problems) = parse(&std::fs::read_to_string(&p).unwrap_or_default());
        for b in &problems {
            crate::problem!("[hotkeys] hotkeys.txt: {b}");
        }
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Mutex::new(bindings.clone()));
        let state = Arc::new(Mutex::new(vec![]));
        let thread = start_thread(shared.clone(), state.clone(), tx.clone());
        Hotkeys {
            bindings,
            state,
            problems,
            rx: Mutex::new(rx),
            tx,
            thread,
            shared,
        }
    }

    pub fn supported() -> bool {
        cfg!(windows)
    }

    fn apply(&mut self) -> Result<(), String> {
        std::fs::write(path(), write(&self.bindings)).map_err(|e| e.to_string())?;
        *self.shared.lock().unwrap() = self.bindings.clone();
        self.reload_thread();
        Ok(())
    }

    #[cfg(any(windows, winchk))]
    fn reload_thread(&self) {
        if let Some(id) = self.thread {
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                    id,
                    windows_sys::Win32::UI::WindowsAndMessaging::WM_APP,
                    0,
                    0,
                );
            }
        }
    }
    #[cfg(not(any(windows, winchk)))]
    fn reload_thread(&self) {}

    /// hotkey:set:<combo>=<command>, hotkey:del:<combo>, hotkey:reload, hotkey:defaults
    pub fn command(&mut self, arg: &str) -> Option<String> {
        let (verb, rest) = arg.split_once(':').unwrap_or((arg, ""));
        let msg = match verb {
            "set" => {
                let (k, c) = rest.split_once('=')?;
                let (keys, mods, vk) = parse_combo(k)?;
                let cmd = c.trim().to_string();
                if cmd.is_empty() {
                    return None;
                }
                self.bindings.retain(|b| !(b.mods == mods && b.vk == vk));
                self.bindings.push(Binding {
                    keys: keys.clone(),
                    mods,
                    vk,
                    cmd: cmd.clone(),
                });
                format!("Hotkey {keys}: {cmd}")
            }
            "del" => {
                let (_, mods, vk) = parse_combo(rest)?;
                let n = self.bindings.len();
                self.bindings.retain(|b| !(b.mods == mods && b.vk == vk));
                if n == self.bindings.len() {
                    return None;
                }
                "Hotkey removed".into()
            }
            "reload" => {
                let (b, bad) = parse(&std::fs::read_to_string(path()).unwrap_or_default());
                self.bindings = b;
                self.problems = bad;
                *self.shared.lock().unwrap() = self.bindings.clone();
                self.reload_thread();
                return Some(format!(
                    "{} hotkeys loaded from hotkeys.txt",
                    self.bindings.len()
                ));
            }
            "defaults" => {
                self.bindings = parse(DEFAULTS).0;
                "Hotkeys back to the defaults".into()
            }
            _ => return None,
        };
        match self.apply() {
            Ok(()) => Some(msg),
            Err(e) => Some(format!("Couldn't save hotkeys.txt: {e}")),
        }
    }

    pub fn json(&self) -> serde_json::Value {
        let st = self.state.lock().unwrap();
        let list: Vec<serde_json::Value> = self
            .bindings
            .iter()
            .map(|b| {
                let s = st.iter().find(|(k, _)| *k == b.keys).map(|(_, r)| r.clone());
                serde_json::json!({"keys": b.keys, "cmd": b.cmd, "ok": matches!(s, Some(Ok(()))), "error": s.and_then(|r| r.err())})
            })
            .collect();
        serde_json::json!({"supported": Self::supported(), "list": list, "problems": self.problems})
    }
}

/// Pressed hotkeys go in with the control page's commands.
pub fn pump(hk: Res<Hotkeys>, mut inject: ResMut<crate::Inject>) {
    for cmd in hk.rx.lock().unwrap().try_iter() {
        crate::note!("[hotkeys] {cmd}");
        inject.0.push(cmd);
    }
}

#[cfg(not(any(windows, winchk)))]
fn start_thread(
    shared: Arc<Mutex<Vec<Binding>>>,
    state: Arc<Mutex<Vec<(String, Result<(), String>)>>>,
    _tx: mpsc::Sender<String>,
) -> Option<u32> {
    *state.lock().unwrap() = shared
        .lock()
        .unwrap()
        .iter()
        .map(|b| {
            (
                b.keys.clone(),
                Err("global hotkeys are Windows only".to_string()),
            )
        })
        .collect();
    None
}

/// A thread with its own message loop: RegisterHotKey for each binding, WM_HOTKEY sends the
/// command, WM_APP re-reads the bindings.
#[cfg(any(windows, winchk))]
fn start_thread(
    shared: Arc<Mutex<Vec<Binding>>>,
    state: Arc<Mutex<Vec<(String, Result<(), String>)>>>,
    tx: mpsc::Sender<String>,
) -> Option<u32> {
    use windows_sys::Win32::{
        System::Threading::GetCurrentThreadId,
        UI::{
            Input::KeyboardAndMouse::{MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey},
            WindowsAndMessaging::{
                GetMessageW, MSG, PM_NOREMOVE, PeekMessageW, WM_APP, WM_HOTKEY, WM_USER,
            },
        },
    };
    let (id_tx, id_rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("hotkeys".into())
        .spawn(move || unsafe {
            let mut msg: MSG = std::mem::zeroed();
            // make sure this thread has a message queue before anyone posts to it
            PeekMessageW(
                &mut msg,
                std::ptr::null_mut(),
                WM_USER,
                WM_USER,
                PM_NOREMOVE,
            );
            let _ = id_tx.send(GetCurrentThreadId());
            let mut live: Vec<Binding> = vec![];
            let register = |live: &mut Vec<Binding>| {
                for i in 0..live.len() {
                    UnregisterHotKey(std::ptr::null_mut(), i as i32 + 1);
                }
                *live = shared.lock().unwrap().clone();
                let mut st = vec![];
                for (i, b) in live.iter().enumerate() {
                    let ok = RegisterHotKey(
                        std::ptr::null_mut(),
                        i as i32 + 1,
                        b.mods | MOD_NOREPEAT,
                        b.vk,
                    ) != 0;
                    if !ok {
                        crate::problem!("[hotkeys] {} is taken by another program", b.keys);
                    }
                    st.push((
                        b.keys.clone(),
                        if ok {
                            Ok(())
                        } else {
                            Err("taken by another program".to_string())
                        },
                    ));
                }
                *state.lock().unwrap() = st;
            };
            register(&mut live);
            loop {
                let r = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
                if r == 0 || r == -1 {
                    break;
                }
                if msg.message == WM_HOTKEY {
                    if let Some(b) = live.get((msg.wParam as usize).wrapping_sub(1)) {
                        if tx.send(b.cmd.clone()).is_err() {
                            break;
                        }
                    }
                } else if msg.message == WM_APP {
                    register(&mut live);
                }
            }
        })
        .ok()?;
    id_rx.recv_timeout(std::time::Duration::from_secs(2)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combos_and_files() {
        assert_eq!(
            parse_combo("ctrl + alt + t"),
            Some(("Ctrl+Alt+T".into(), MOD_CTRL | MOD_ALT, b'T' as u32))
        );
        assert_eq!(
            parse_combo("Shift+F5"),
            Some(("Shift+F5".into(), MOD_SHIFT, 0x74))
        );
        assert_eq!(
            parse_combo("F9").map(|c| c.2),
            Some(0x78),
            "F keys alone are fine"
        );
        assert!(
            parse_combo("T").is_none(),
            "a bare letter would eat your typing"
        );
        assert!(parse_combo("Ctrl+Hyper+T").is_none());
        assert_eq!(
            parse_combo("alt+pagedown").map(|c| c.0),
            Some("Alt+PageDown".into())
        );
        assert_eq!(parse_combo("Ctrl+Num7").map(|c| c.2), Some(0x67));
        let (b, bad) = parse(DEFAULTS);
        assert!(bad.is_empty(), "{bad:?}");
        assert!(b.len() >= 12);
        let (b, bad) =
            parse("Ctrl+= = emote:hop\nCtrl+Alt+T = a\nctrl+alt+t = b\nnonsense\nQ = x\n");
        assert_eq!(b.len(), 2, "{b:?}");
        assert_eq!(b[0].vk, 0xBB);
        assert_eq!(b[1].cmd, "b", "a later line wins");
        assert_eq!(bad.len(), 2);
        // and back out
        let (again, _) = parse(&write(&b));
        assert_eq!(again, b);
    }
}
