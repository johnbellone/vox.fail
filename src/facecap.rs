//! Live face capture from a face-tracking app, mapped onto the block face's parts.
//!
//! The app listens (on this computer only) for the two common formats trackers send:
//! - **VMC** (OSC over UDP, port 39539): VSeeFace, Warudo, VNyan, iFacialMocap's PC relay and
//!   others. Blend shapes (`/VMC/Ext/Blend/Val` + `/Apply`; ARKit "perfect sync" names or the VRM
//!   ones: Blink, A I U E O, Joy, Angry, Sorrow, Fun) and the head bone (`/VMC/Ext/Bone/Pos`).
//! - **iFacialMocap** (UDP, port 49983): the iPhone app sends straight to this computer's
//!   address: `eyeBlink_L-12|jawOpen-40|...=head#pitch,yaw,roll,...` (values 0-100, degrees).
//!
//! The tracker's numbers pick the face parts: blinks shut the eyes, wide or squinted eyes, a
//! look to the side or up, brows up (inner only: worried) or down, and the mouth's shape (smile,
//! one-sided smirk, frown, pout, an "o", the tongue). How open the mouth is still comes from
//! the voice, which keeps lip sync tight; with the mic quiet, an open jaw opens the mouth. The
//! head turns, nods and tilts with yours on top of everything else.
//!
//! `facecap:on` toggles it (`face_capture=false` in the settings turns it off at start);
//! `facecap_vmc_port` / `facecap_ifm_port` change the ports.

use std::{
    collections::HashMap,
    net::UdpSocket,
    sync::{
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    time::Duration,
};

use bevy::prelude::*;

/// One reading from a tracker: blend shapes (canonical ARKit names in lower case, 0..1) and
/// the head's rotation (radians: x nods down, y turns to the character's left, z tilts).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reading {
    pub shapes: HashMap<String, f32>,
    pub head: Option<Vec3>,
}

/// ARKit names in lower case with "left"/"right" spelled out ("eyeBlink_L" -> "eyeblinkleft").
pub fn canonical(name: &str) -> String {
    let n = name.trim().to_ascii_lowercase();
    let n = n.strip_suffix("_l").map(|s| format!("{s}left")).or_else(|| n.strip_suffix("_r").map(|s| format!("{s}right"))).unwrap_or(n);
    n.replace('_', "")
}

/// The VRM preset names (what plain webcam trackers send) in ARKit terms.
fn from_vrm(name: &str, v: f32, out: &mut HashMap<String, f32>) -> bool {
    let mut put = |k: &str, x: f32| {
        let e = out.entry(k.to_string()).or_insert(0.0);
        *e = e.max(x);
    };
    match name.to_ascii_lowercase().as_str() {
        "blink" => {
            put("eyeblinkleft", v);
            put("eyeblinkright", v);
        }
        "blink_l" => put("eyeblinkleft", v),
        "blink_r" => put("eyeblinkright", v),
        "a" => put("jawopen", v),
        "o" | "u" => {
            put("jawopen", v * 0.4);
            put("mouthfunnel", v);
        }
        "i" | "e" => put("jawopen", v * 0.3),
        "joy" | "fun" => {
            put("mouthsmileleft", v);
            put("mouthsmileright", v);
        }
        "angry" => {
            put("browdownleft", v);
            put("browdownright", v);
        }
        "sorrow" => {
            put("browinnerup", v);
            put("mouthfrownleft", v);
            put("mouthfrownright", v);
        }
        "surprised" => {
            put("eyewideleft", v);
            put("eyewideright", v);
            put("browinnerup", v);
            put("jawopen", v * 0.5);
        }
        _ => return false,
    }
    true
}

/// An iFacialMocap packet: `name-value|...=head#x,y,z,tx,ty,tz|rightEye#..|leftEye#..` (newer
/// versions use `&` between name and value). Values 0-100, rotations in degrees.
pub fn parse_ifm(text: &str) -> Option<Reading> {
    let (shapes, rest) = text.split_once('=').unwrap_or((text, ""));
    let mut r = Reading::default();
    for item in shapes.split('|').filter(|s| !s.is_empty()) {
        let (name, val) = item.rsplit_once('&').or_else(|| item.rsplit_once('-'))?;
        let v: f32 = val.trim().parse().ok()?;
        r.shapes.insert(canonical(name), (v / 100.0).clamp(0.0, 1.0));
    }
    for part in rest.split('|') {
        if let Some(vals) = part.strip_prefix("head#") {
            let n: Vec<f32> = vals.split(',').filter_map(|v| v.trim().parse().ok()).collect();
            if n.len() >= 3 {
                // the phone faces you: its pitch is a nod, its yaw a turn (mirrored), its roll a tilt
                r.head = Some(Vec3::new(n[0].to_radians(), -n[1].to_radians(), -n[2].to_radians()));
            }
        }
    }
    (!r.shapes.is_empty() || r.head.is_some()).then_some(r)
}

/// One OSC argument.
#[derive(Clone, Debug, PartialEq)]
pub enum OscArg {
    Str(String),
    Float(f32),
    Int(i32),
}

/// OSC messages in a packet (a message, or a bundle of them, nested): (address, arguments).
pub fn parse_osc(buf: &[u8]) -> Vec<(String, Vec<OscArg>)> {
    let mut out = vec![];
    osc_into(buf, &mut out, 0);
    out
}

fn osc_str(buf: &[u8], at: &mut usize) -> Option<String> {
    let start = *at;
    let end = start + buf.get(start..)?.iter().position(|b| *b == 0)?;
    let s = String::from_utf8_lossy(&buf[start..end]).into_owned();
    *at = (end + 4) & !3;
    Some(s)
}

fn osc_into(buf: &[u8], out: &mut Vec<(String, Vec<OscArg>)>, depth: u32) {
    if depth > 8 {
        return;
    }
    if buf.starts_with(b"#bundle\0") {
        let mut at = 16; // "#bundle\0" + 8-byte time tag
        while at + 4 <= buf.len() {
            let n = u32::from_be_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]]) as usize;
            at += 4;
            let Some(el) = buf.get(at..at + n) else { return };
            osc_into(el, out, depth + 1);
            at += n;
        }
        return;
    }
    let mut at = 0;
    let Some(addr) = osc_str(buf, &mut at) else { return };
    let Some(tags) = osc_str(buf, &mut at) else { return };
    let mut args = vec![];
    for t in tags.chars().skip(1) {
        match t {
            'f' | 'i' => {
                let Some(b) = buf.get(at..at + 4) else { return };
                let raw = [b[0], b[1], b[2], b[3]];
                args.push(if t == 'f' { OscArg::Float(f32::from_be_bytes(raw)) } else { OscArg::Int(i32::from_be_bytes(raw)) });
                at += 4;
            }
            's' => {
                let Some(s) = osc_str(buf, &mut at) else { return };
                args.push(OscArg::Str(s));
            }
            _ => return, // a type we don't read: skip the message
        }
    }
    out.push((addr, args));
}

/// VMC messages build up a reading; `Apply` hands it over.
#[derive(Default)]
pub struct VmcState {
    pending: HashMap<String, f32>,
    head: Option<Vec3>,
}

impl VmcState {
    pub fn feed(&mut self, msgs: &[(String, Vec<OscArg>)]) -> Option<Reading> {
        let mut done = None;
        for (addr, args) in msgs {
            match (addr.as_str(), args.as_slice()) {
                ("/VMC/Ext/Blend/Val", [OscArg::Str(name), OscArg::Float(v)]) => {
                    if !from_vrm(name, *v, &mut self.pending) {
                        self.pending.insert(canonical(name), v.clamp(0.0, 1.0));
                    }
                }
                ("/VMC/Ext/Blend/Apply", _) => {
                    done = Some(Reading { shapes: std::mem::take(&mut self.pending), head: self.head });
                }
                ("/VMC/Ext/Bone/Pos", [OscArg::Str(bone), _, _, _, OscArg::Float(x), OscArg::Float(y), OscArg::Float(z), OscArg::Float(w), ..]) if bone == "Head" => {
                    // Unity's left-handed rotation to ours: flip the yaw and the roll
                    let (ex, ey, ez) = Quat::from_xyzw(*x, *y, *z, *w).to_euler(EulerRot::YXZ);
                    self.head = Some(Vec3::new(ey, -ex, -ez));
                }
                _ => {}
            }
        }
        done
    }
}

/// What the face should show for a reading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Choice {
    pub eyes: &'static str,
    pub brows: &'static str,
    /// a mouth shape the face is making (None: leave it to the lip sync)
    pub mouth: Option<&'static str>,
    pub jaw: f32,
}

pub fn choose(shapes: &HashMap<String, f32>) -> Choice {
    let g = |k: &str| shapes.get(k).copied().unwrap_or(0.0);
    let both = |a: &str| (g(&format!("{a}left")) + g(&format!("{a}right"))) * 0.5;
    let blink = both("eyeblink");
    let look_l = (g("eyelookoutright") + g("eyelookinleft")) * 0.5; // looking to your right = the character's left
    let look_r = (g("eyelookoutleft") + g("eyelookinright")) * 0.5;
    let look_up = both("eyelookup");
    let eyes = if blink > 0.55 {
        "eyes_shut"
    } else if both("eyewide") > 0.45 {
        "eyes_wide"
    } else if both("eyesquint") > 0.5 || blink > 0.35 {
        "eyes_squint"
    } else if look_up > 0.45 {
        "eyes_up"
    } else if look_l > 0.45 && look_l > look_r {
        "eyes_left"
    } else if look_r > 0.45 {
        "eyes_right"
    } else {
        "eyes_open"
    };
    let inner = g("browinnerup");
    let down = both("browdown");
    let outer = both("browouterup");
    let brows = if down > 0.4 && down > inner {
        "brows_angry"
    } else if inner > 0.45 && outer < 0.3 {
        "brows_worried"
    } else if inner > 0.35 || outer > 0.35 {
        "brows_raised"
    } else {
        "brows_normal"
    };
    let (sl, sr) = (g("mouthsmileleft"), g("mouthsmileright"));
    let jaw = g("jawopen");
    let mouth = if g("tongueout") > 0.4 {
        Some("mouth_tongue")
    } else if g("mouthfunnel") > 0.45 {
        Some("mouth_o")
    } else if g("mouthpucker") > 0.5 {
        Some("mouth_pout")
    } else if sl.max(sr) > 0.45 && (sl - sr).abs() > 0.3 {
        Some("mouth_smirk")
    } else if (sl + sr) * 0.5 > 0.45 {
        Some(if jaw > 0.3 { "mouth_open" } else { "mouth_grin" })
    } else if both("mouthfrown") > 0.4 {
        Some(if both("mouthfrown") > 0.65 { "mouth_sad" } else { "mouth_frown" })
    } else {
        None
    };
    Choice { eyes, brows, mouth, jaw }
}

#[derive(Resource)]
pub struct FaceCap {
    pub on: bool,
    rx: Option<Mutex<Receiver<(&'static str, Reading)>>>,
    pub status: Arc<Mutex<String>>,
    /// the newest reading, when it came (app seconds) and from which format
    latest: Option<(f32, &'static str, Reading)>,
    /// the head turn, smoothed
    head: Vec3,
    /// turn and tilt the other way (trackers disagree on which side is which)
    pub mirror: bool,
}

/// How long a reading counts: after that the face goes back to running on its own.
const STALE: f32 = 0.6;

impl FaceCap {
    pub fn new(on: bool, vmc_port: u16, ifm_port: u16) -> Self {
        let status = Arc::new(Mutex::new(String::new()));
        // listen either way, so turning it on later needs no restart (the ports are only read)
        let rx = Some(Mutex::new(start(vmc_port, ifm_port, status.clone())));
        *status.lock().unwrap_or_else(|e| e.into_inner()) = if on { format!("Face capture: waiting for a tracker (VMC on port {vmc_port}, iFacialMocap on {ifm_port})") } else { "Face capture off".into() };
        FaceCap { on, rx, status, latest: None, head: Vec3::ZERO, mirror: false }
    }

    /// The reading to use now, if a tracker is sending.
    pub fn live(&self, now: f32) -> Option<(&Reading, Vec3)> {
        let (t, _, r) = self.latest.as_ref()?;
        (self.on && now - t < STALE).then_some((r, self.head))
    }

    /// facecap:on (toggle), facecap:mirror (toggle)
    pub fn command(&mut self, arg: &str) -> Option<String> {
        match arg {
            "on" => {
                self.on = !self.on;
                Some(format!("Face capture {}", if self.on { "on" } else { "off" }))
            }
            "mirror" => {
                self.mirror = !self.mirror;
                Some(format!("Face capture head turn {}", if self.mirror { "mirrored" } else { "as tracked" }))
            }
            _ => None,
        }
    }

    pub fn json(&self, now: f32) -> serde_json::Value {
        let from = self.latest.as_ref().filter(|(t, _, _)| now - t < STALE).map(|(_, f, _)| *f);
        serde_json::json!({"on": self.on, "mirror": self.mirror, "tracking": from, "status": *self.status.lock().unwrap_or_else(|e| e.into_inner())})
    }
}

/// The two listener threads.
fn start(vmc_port: u16, ifm_port: u16, status: Arc<Mutex<String>>) -> Receiver<(&'static str, Reading)> {
    let (tx, rx) = mpsc::channel();
    let listen = |port: u16, name: &'static str, tx: Sender<(&'static str, Reading)>, status: Arc<Mutex<String>>| {
        std::thread::Builder::new()
            .name(format!("facecap-{name}"))
            .spawn(move || {
                let Ok(sock) = UdpSocket::bind(("0.0.0.0", port)) else {
                    crate::note!("[facecap] can't listen for {name} on port {port}");
                    *status.lock().unwrap_or_else(|e| e.into_inner()) = format!("Face capture: port {port} ({name}) is taken by another app");
                    return;
                };
                let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = vec![0u8; 65536];
                let mut vmc = VmcState::default();
                loop {
                    let Ok(n) = sock.recv(&mut buf) else { continue };
                    let reading = if name == "VMC" { vmc.feed(&parse_osc(&buf[..n])) } else { std::str::from_utf8(&buf[..n]).ok().and_then(parse_ifm) };
                    if let Some(r) = reading {
                        if tx.send((name, r)).is_err() {
                            return;
                        }
                    }
                }
            })
            .ok();
    };
    listen(vmc_port, "VMC", tx.clone(), status.clone());
    listen(ifm_port, "iFacialMocap", tx, status);
    rx
}

pub fn update(mut cap: ResMut<FaceCap>, time: Res<Time>) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let got: Vec<(&'static str, Reading)> = cap.rx.as_ref().map(|rx| rx.lock().unwrap_or_else(|e| e.into_inner()).try_iter().collect()).unwrap_or_default();
    if let Some((from, r)) = got.into_iter().last() {
        let first = cap.latest.as_ref().is_none_or(|(t, _, _)| now - t > STALE);
        if first {
            *cap.status.lock().unwrap_or_else(|e| e.into_inner()) = format!("Face capture: tracking ({from})");
        }
        cap.latest = Some((now, from, r));
    }
    // follow the tracked head smoothly (trackers jitter), and let go when it stops
    let flip = if cap.mirror { Vec3::new(1.0, -1.0, -1.0) } else { Vec3::ONE };
    let want = cap.live(now).and_then(|(r, _)| r.head).map(|h| (h * flip).clamp(Vec3::splat(-0.7), Vec3::splat(0.7))).unwrap_or(Vec3::ZERO);
    let k = 1.0 - (-dt * 14.0).exp();
    let head = cap.head;
    cap.head = head + (want - head) * k;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn osc(addr: &str, args: &[OscArg]) -> Vec<u8> {
        let pad = |b: &mut Vec<u8>, s: &str| {
            b.extend_from_slice(s.as_bytes());
            b.push(0);
            while b.len() % 4 != 0 {
                b.push(0);
            }
        };
        let mut b = vec![];
        pad(&mut b, addr);
        let tags: String = std::iter::once(',').chain(args.iter().map(|a| match a {
            OscArg::Str(_) => 's',
            OscArg::Float(_) => 'f',
            OscArg::Int(_) => 'i',
        })).collect();
        pad(&mut b, &tags);
        for a in args {
            match a {
                OscArg::Str(s) => pad(&mut b, s),
                OscArg::Float(f) => b.extend_from_slice(&f.to_be_bytes()),
                OscArg::Int(i) => b.extend_from_slice(&i.to_be_bytes()),
            }
        }
        b
    }

    fn bundle(msgs: &[Vec<u8>]) -> Vec<u8> {
        let mut b = b"#bundle\0".to_vec();
        b.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1]);
        for m in msgs {
            b.extend_from_slice(&(m.len() as u32).to_be_bytes());
            b.extend_from_slice(m);
        }
        b
    }

    #[test]
    fn names() {
        assert_eq!(canonical("eyeBlink_L"), "eyeblinkleft");
        assert_eq!(canonical("mouthSmileRight"), "mouthsmileright");
        assert_eq!(canonical("jawOpen"), "jawopen");
    }

    #[test]
    fn reads_ifacialmocap() {
        let r = parse_ifm("eyeBlink_L-90|eyeBlink_R-88|jawOpen-40|mouthSmile_L&70=head#-10.5,20,5,0,0,0|rightEye#1,2,3|leftEye#1,2,3").unwrap();
        assert!((r.shapes["eyeblinkleft"] - 0.9).abs() < 1e-6 && (r.shapes["mouthsmileleft"] - 0.7).abs() < 1e-6);
        let h = r.head.unwrap();
        assert!((h.x - (-10.5f32).to_radians()).abs() < 1e-5 && (h.y + 20f32.to_radians()).abs() < 1e-5);
        assert!(parse_ifm("garbage").is_none());
        assert_eq!(choose(&r.shapes).eyes, "eyes_shut");
    }

    #[test]
    fn reads_vmc_bundles() {
        let pkt = bundle(&[
            osc("/VMC/Ext/Blend/Val", &[OscArg::Str("Joy".into()), OscArg::Float(0.8)]),
            osc("/VMC/Ext/Blend/Val", &[OscArg::Str("browInnerUp".into()), OscArg::Float(0.6)]),
            osc("/VMC/Ext/Bone/Pos", &[OscArg::Str("Head".into()), OscArg::Float(0.0), OscArg::Float(1.5), OscArg::Float(0.0), OscArg::Float(0.0), OscArg::Float(0.0), OscArg::Float(0.0), OscArg::Float(1.0)]),
        ]);
        let msgs = parse_osc(&pkt);
        assert_eq!(msgs.len(), 3);
        let mut st = VmcState::default();
        assert!(st.feed(&msgs).is_none(), "nothing until Apply");
        let r = st.feed(&parse_osc(&osc("/VMC/Ext/Blend/Apply", &[]))).unwrap();
        assert!((r.shapes["mouthsmileleft"] - 0.8).abs() < 1e-6, "VRM Joy is a smile");
        assert_eq!(r.head, Some(Vec3::ZERO));
        let c = choose(&r.shapes);
        assert_eq!((c.brows, c.mouth), ("brows_worried", Some("mouth_grin")));
        // a malformed packet is ignored, not a crash
        assert!(parse_osc(&[1, 2, 3]).is_empty());
        assert!(parse_osc(b"#bundle\0\0\0\0\0\0\0\0\x01\0\0\0\xff").is_empty());
    }

    #[test]
    fn expressions() {
        let s = |kv: &[(&str, f32)]| kv.iter().map(|(k, v)| (k.to_string(), *v)).collect::<HashMap<_, _>>();
        assert_eq!(choose(&s(&[])), Choice { eyes: "eyes_open", brows: "brows_normal", mouth: None, jaw: 0.0 });
        assert_eq!(choose(&s(&[("browdownleft", 0.7), ("browdownright", 0.7)])).brows, "brows_angry");
        assert_eq!(choose(&s(&[("mouthsmileleft", 0.8), ("mouthsmileright", 0.1)])).mouth, Some("mouth_smirk"));
        assert_eq!(choose(&s(&[("mouthfunnel", 0.7)])).mouth, Some("mouth_o"));
        assert_eq!(choose(&s(&[("tongueout", 0.9)])).mouth, Some("mouth_tongue"));
        assert_eq!(choose(&s(&[("eyewideleft", 0.6), ("eyewideright", 0.6)])).eyes, "eyes_wide");
        assert_eq!(choose(&s(&[("eyelookupleft", 0.6), ("eyelookupright", 0.6)])).eyes, "eyes_up");
    }

    /// The listener end to end: an iFacialMocap packet sent to the port becomes a reading.
    #[test]
    fn listens_on_udp() {
        let port = 49000 + (std::process::id() % 900) as u16;
        let status = Arc::new(Mutex::new(String::new()));
        let rx = start(port + 1000, port, status);
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        let mut got = None;
        for _ in 0..20 {
            s.send_to(b"jawOpen-55=head#0,0,0,0,0,0", ("127.0.0.1", port)).unwrap();
            if let Ok(r) = rx.recv_timeout(Duration::from_millis(200)) {
                got = Some(r);
                break;
            }
        }
        let (from, r) = got.expect("a reading");
        assert_eq!(from, "iFacialMocap");
        assert!((r.shapes["jawopen"] - 0.55).abs() < 1e-6);
    }
}
