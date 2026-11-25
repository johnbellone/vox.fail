//! Virtual webcam output: a second camera renders the avatar off-screen at 1280x720 (or any size),
//! the frames are copied back from the GPU, converted to NV12 on a worker thread and handed to a
//! virtual camera driver at a steady 60 fps.
//!
//! Driver: OBS Studio's virtual camera on Windows. Installing OBS registers a DirectShow camera
//! called "OBS Virtual Camera" that reads frames from a shared-memory queue named
//! `OBSVirtualCamVideo`. This module writes that queue exactly as OBS itself does
//! (plugins/win-dshow/shared-memory-queue.c), so OBS doesn't have to be running, but OBS's own
//! virtual camera must be stopped while this one is on.
//!
//! The camera reader drops back to OBS's placeholder picture if it sees the same frame ten times
//! in a row, so the worker re-sends the last frame whenever the renderer stalls (loading a model,
//! a minimised window), keeping Zoom/Discord/Meet from flickering to the placeholder.

use std::{
    sync::{
        Arc, Mutex,
        mpsc::{self, RecvTimeoutError, SyncSender},
    },
    time::{Duration, Instant},
};

use bevy::{
    camera::{ClearColorConfig, RenderTarget},
    core_pipeline::tonemapping::Tonemapping,
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{TextureFormat, TextureUsages},
    },
};

/// Where finished frames go.
#[derive(Clone, Debug)]
pub enum Sink {
    /// OBS's virtual camera driver (Windows)
    ObsVirtualCamera,
    /// raw NV12 frames appended to a file, for testing: `ffmpeg -f rawvideo -pix_fmt nv12 -s WxH -r 60 -i file`
    Dump(String),
}

#[derive(Default)]
pub struct Stats {
    pub sent_fps: f32,
    pub error: Option<String>,
    pub running: bool,
}

#[derive(Resource)]
pub struct Webcam {
    pub enabled: bool,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub sink: Sink,
    pub stats: Arc<Mutex<Stats>>,
    image: Option<Handle<Image>>,
    camera: Option<Entity>,
    tx: Option<SyncSender<Vec<u8>>>,
    next_due: Option<Instant>,
}

#[derive(Component)]
pub struct WebcamCamera;

impl Webcam {
    pub fn new(enabled: bool, width: u32, height: u32, fps: u32, sink: Sink) -> Self {
        // NV12 needs even dimensions
        Webcam {
            enabled,
            width: width.max(2) & !1,
            height: height.max(2) & !1,
            fps: fps.clamp(1, 240),
            sink,
            stats: Arc::new(Mutex::new(Stats::default())),
            image: None,
            camera: None,
            tx: None,
            next_due: None,
        }
    }

    pub fn camera_entity(&self) -> Option<Entity> {
        self.camera
    }

    pub fn image_handle(&self) -> Option<Handle<Image>> {
        self.image.clone()
    }

    pub fn sink_name(&self) -> String {
        match &self.sink {
            Sink::ObsVirtualCamera => "OBS Virtual Camera".into(),
            Sink::Dump(p) => format!("file {p}"),
        }
    }

    /// One line for the HUD.
    pub fn status_line(&self) -> String {
        if !self.enabled {
            return "Webcam: off (W to turn on)".into();
        }
        let s = self.stats.lock().unwrap();
        if let Some(e) = &s.error {
            return format!("Webcam: {e}");
        }
        format!(
            "Webcam: {} {}x{} @ {} fps, sending {:.1}",
            self.sink_name(),
            self.width,
            self.height,
            self.fps,
            s.sent_fps
        )
    }

    fn start_worker(&mut self) {
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(1);
        self.tx = Some(tx);
        self.next_due = None;
        let (w, h, fps, sink, stats) = (
            self.width as usize,
            self.height as usize,
            self.fps,
            self.sink.clone(),
            self.stats.clone(),
        );
        *stats.lock().unwrap() = Stats::default();
        std::thread::Builder::new()
            .name("webcam".into())
            .spawn(move || {
                let interval = Duration::from_secs_f64(1.0 / fps as f64);
                let mut out: Box<dyn FrameOut> = match open_sink(&sink, w as u32, h as u32, fps) {
                    Ok(o) => o,
                    Err(e) => {
                        crate::problem!("[webcam] {e}");
                        stats.lock().unwrap().error = Some(e);
                        return;
                    }
                };
                crate::note!("[webcam] sending {w}x{h} @ {fps} fps");
                stats.lock().unwrap().running = true;
                let mut nv12 = vec![0u8; w * h * 3 / 2];
                let mut have_frame = false;
                let start = Instant::now();
                let (mut count, mut window) = (0u32, Instant::now());
                loop {
                    match rx.recv_timeout(interval * 2) {
                        Ok(rgba) => {
                            rgba_to_nv12(&rgba, w, h, &mut nv12);
                            have_frame = true;
                            count += 1;
                        }
                        // renderer stalled: repeat the last frame so the reader doesn't give up
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                    if have_frame {
                        if let Err(e) = out.write(&nv12, start.elapsed().as_nanos() as u64) {
                            stats.lock().unwrap().error = Some(e);
                            break;
                        }
                    }
                    let el = window.elapsed().as_secs_f32();
                    if el >= 1.0 {
                        stats.lock().unwrap().sent_fps = count as f32 / el;
                        count = 0;
                        window = Instant::now();
                    }
                }
                stats.lock().unwrap().running = false;
                crate::note!("[webcam] stopped");
            })
            .expect("spawn webcam thread");
    }
}

// ------------------------------------------------------------------------------------------
// Bevy side
// ------------------------------------------------------------------------------------------

/// Startup: the off-screen render target and the camera that draws into it.
pub fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut cam: ResMut<Webcam>) {
    let mut image =
        Image::new_target_texture(cam.width, cam.height, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC | TextureUsages::RENDER_ATTACHMENT;
    let handle = images.add(image);
    let e = commands
        .spawn((
            Camera3d::default(),
            Camera {
                order: -3,
                is_active: cam.enabled,
                clear_color: ClearColorConfig::Custom(Color::srgb_u8(0, 177, 64)),
                ..default()
            },
            RenderTarget::Image(handle.clone().into()),
            Tonemapping::None,
            Transform::default(),
            WebcamCamera,
        ))
        .observe(on_frame)
        .id();
    if cam.enabled {
        commands.entity(e).insert(Readback::texture(handle.clone()));
        cam.start_worker();
    }
    cam.image = Some(handle);
    cam.camera = Some(e);
}

/// Turn the output on or off.
pub fn set_enabled(cam: &mut Webcam, on: bool, commands: &mut Commands) {
    if cam.enabled == on {
        return;
    }
    cam.enabled = on;
    let (Some(e), Some(img)) = (cam.camera, cam.image.clone()) else {
        return;
    };
    if on {
        commands.entity(e).insert(Readback::texture(img));
        cam.start_worker();
    } else {
        commands.entity(e).remove::<Readback>();
        cam.tx = None; // worker sees the channel close, marks the queue stopped and exits
    }
}

/// Keep the webcam camera on the same view as the window camera and give it a solid background
/// (a webcam can't carry transparency, so transparent mode sends green).
pub fn follow(
    mut cams: Query<(&mut Transform, &mut Camera), With<WebcamCamera>>,
    src: Query<&Transform, (With<crate::MainCamera>, Without<WebcamCamera>)>,
    cam: Res<Webcam>,
    clear: Res<ClearColor>,
    stages: Res<crate::stage::Stages>,
) {
    let Ok(src) = src.single() else { return };
    for (mut t, mut c) in &mut cams {
        *t = *src;
        c.is_active = cam.enabled;
        let bg = if clear.0.alpha() < 1.0 {
            Color::srgb_u8(0, 177, 64)
        } else {
            clear.0
        };
        if !stages.retro {
            c.clear_color = ClearColorConfig::Custom(bg);
        }
    }
}

/// A frame came back from the GPU: pace it to the target rate and hand it to the worker.
fn on_frame(ev: On<ReadbackComplete>, mut cam: ResMut<Webcam>) {
    let Some(tx) = cam.tx.clone() else { return };
    let now = Instant::now();
    let interval = Duration::from_secs_f64(1.0 / cam.fps as f64);
    let due = *cam.next_due.get_or_insert(now);
    // 2 ms early is close enough; a 144 Hz display then sends every 2nd or 3rd frame but averages 60
    if now + Duration::from_millis(2) < due {
        return;
    }
    // advance by whole intervals so the average rate is exact; resync if we fell far behind
    let mut next = due + interval;
    if next + interval < now {
        next = now + interval;
    }
    cam.next_due = Some(next);
    let expected = (cam.width * cam.height * 4) as usize;
    let data = if ev.data.len() == expected {
        ev.data.clone()
    } else {
        strip_padding(&ev.data, cam.width as usize, cam.height as usize)
    };
    let _ = tx.try_send(data); // worker busy: drop this one, the next is 16 ms away
}

/// GPU copies pad each row to 256 bytes; take that back out if the width needed it.
pub(crate) fn strip_padding(data: &[u8], w: usize, h: usize) -> Vec<u8> {
    let row = w * 4;
    let stride = if h > 0 { data.len() / h } else { row };
    let mut out = Vec::with_capacity(row * h);
    for y in 0..h {
        let s = y * stride;
        out.extend_from_slice(&data[s..(s + row).min(data.len())]);
    }
    out.resize(row * h, 0);
    out
}

// ------------------------------------------------------------------------------------------
// Colour conversion: sRGB RGBA -> NV12 (BT.601, limited range, as webcams send)
// ------------------------------------------------------------------------------------------
pub fn rgba_to_nv12(rgba: &[u8], w: usize, h: usize, out: &mut [u8]) {
    let (y_plane, uv_plane) = out.split_at_mut(w * h);
    for y in 0..h {
        let src = &rgba[y * w * 4..(y + 1) * w * 4];
        let dst = &mut y_plane[y * w..(y + 1) * w];
        for (d, p) in dst.iter_mut().zip(src.chunks_exact(4)) {
            let (r, g, b) = (p[0] as i32, p[1] as i32, p[2] as i32);
            *d = (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16) as u8;
        }
    }
    for y in 0..h / 2 {
        let r0 = &rgba[(2 * y) * w * 4..(2 * y + 1) * w * 4];
        let r1 = &rgba[(2 * y + 1) * w * 4..(2 * y + 2) * w * 4];
        let dst = &mut uv_plane[y * w..(y + 1) * w];
        for x in 0..w / 2 {
            let i = x * 8;
            let r = (r0[i] as i32 + r0[i + 4] as i32 + r1[i] as i32 + r1[i + 4] as i32 + 2) >> 2;
            let g = (r0[i + 1] as i32 + r0[i + 5] as i32 + r1[i + 1] as i32 + r1[i + 5] as i32 + 2)
                >> 2;
            let b = (r0[i + 2] as i32 + r0[i + 6] as i32 + r1[i + 2] as i32 + r1[i + 6] as i32 + 2)
                >> 2;
            dst[2 * x] = (((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128).clamp(0, 255) as u8;
            dst[2 * x + 1] = (((112 * r - 94 * g - 18 * b + 128) >> 8) + 128).clamp(0, 255) as u8;
        }
    }
}

// ------------------------------------------------------------------------------------------
// Sinks
// ------------------------------------------------------------------------------------------
trait FrameOut: Send {
    fn write(&mut self, nv12: &[u8], ts_ns: u64) -> Result<(), String>;
}

fn open_sink(sink: &Sink, w: u32, h: u32, fps: u32) -> Result<Box<dyn FrameOut>, String> {
    match sink {
        Sink::Dump(path) => {
            let f = std::fs::File::create(path).map_err(|e| format!("can't create {path}: {e}"))?;
            Ok(Box::new(DumpOut(std::io::BufWriter::new(f))))
        }
        Sink::ObsVirtualCamera => {
            if !cfg!(windows) {
                return Err(
                    "the virtual camera driver output is Windows-only (OBS Virtual Camera)".into(),
                );
            }
            let interval = 10_000_000u64 / fps as u64; // 100 ns units, as DirectShow uses
            obs_queue::remember_format(w, h, interval);
            Ok(Box::new(obs_queue::ObsQueue::create(w, h, interval)?))
        }
    }
}

struct DumpOut(std::io::BufWriter<std::fs::File>);

impl FrameOut for DumpOut {
    fn write(&mut self, nv12: &[u8], _ts: u64) -> Result<(), String> {
        use std::io::Write;
        self.0.write_all(nv12).map_err(|e| e.to_string())
    }
}

impl FrameOut for obs_queue::ObsQueue {
    fn write(&mut self, nv12: &[u8], ts_ns: u64) -> Result<(), String> {
        self.write_frame(nv12, ts_ns);
        Ok(())
    }
}

/// Writer side of OBS's `OBSVirtualCamVideo` shared-memory queue, laid out byte for byte like
/// `struct queue_header` in obs-studio/plugins/win-dshow/shared-memory-queue.c: a header, then
/// three slots of [32-byte frame header holding a u64 timestamp][NV12 frame], each 32-byte aligned.
pub mod obs_queue {
    use super::sys;
    use std::sync::atomic::{Ordering, fence};

    pub const NAME: &str = "OBSVirtualCamVideo";
    pub const STATE_STARTING: u32 = 1;
    pub const STATE_READY: u32 = 2;
    pub const STATE_STOPPING: u32 = 3;
    pub const FRAME_HEADER_SIZE: usize = 32;

    #[repr(C)]
    pub struct QueueHeader {
        pub write_idx: u32,
        pub read_idx: u32,
        pub state: u32,
        pub offsets: [u32; 3],
        pub kind: u32,
        pub cx: u32,
        pub cy: u32,
        pub interval: u64,
        pub reserved: [u32; 8],
    }

    fn align32(x: usize) -> usize {
        (x + 31) & !31
    }

    /// Slot offsets and total mapping size, computed the way video_queue_create does.
    pub fn layout(cx: u32, cy: u32) -> ([u32; 3], usize) {
        let frame = (cx * cy * 3 / 2) as usize;
        let mut size = align32(std::mem::size_of::<QueueHeader>());
        let mut offs = [0u32; 3];
        for o in &mut offs {
            *o = size as u32;
            size = align32(size + frame + FRAME_HEADER_SIZE);
        }
        (offs, size)
    }

    pub fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub struct ObsQueue {
        handle: sys::Handle,
        base: *mut u8,
        offsets: [u32; 3],
        frame_size: usize,
    }

    // The mapping is only touched from the worker thread that owns it.
    unsafe impl Send for ObsQueue {}

    impl ObsQueue {
        pub fn create(cx: u32, cy: u32, interval: u64) -> Result<Self, String> {
            let name = wide(NAME);
            let (offsets, size) = layout(cx, cy);
            unsafe {
                // someone (OBS's own virtual camera, or a second copy of this app) already owns it
                let existing = sys::OpenFileMappingW(sys::FILE_MAP_READ, 0, name.as_ptr());
                if existing != 0 {
                    sys::CloseHandle(existing);
                    return Err("OBS Virtual Camera is already in use; stop it in OBS (or close the other avatar) and press W".into());
                }
                let handle = sys::CreateFileMappingW(
                    sys::INVALID_HANDLE_VALUE,
                    std::ptr::null(),
                    sys::PAGE_READWRITE,
                    0,
                    size as u32,
                    name.as_ptr(),
                );
                if handle == 0 {
                    return Err("couldn't create the virtual camera queue".into());
                }
                let base = sys::MapViewOfFile(handle, sys::FILE_MAP_ALL_ACCESS, 0, 0, 0) as *mut u8;
                if base.is_null() {
                    sys::CloseHandle(handle);
                    return Err("couldn't map the virtual camera queue".into());
                }
                std::ptr::write_bytes(base, 0, std::mem::size_of::<QueueHeader>());
                let h = base as *mut QueueHeader;
                (*h).state = STATE_STARTING;
                (*h).offsets = offsets;
                (*h).kind = 0; // SHARED_QUEUE_TYPE_VIDEO
                (*h).cx = cx;
                (*h).cy = cy;
                (*h).interval = interval;
                Ok(ObsQueue {
                    handle,
                    base,
                    offsets,
                    frame_size: (cx * cy * 3 / 2) as usize,
                })
            }
        }

        /// Same steps as video_queue_write: bump write_idx, fill that slot, publish it via read_idx.
        pub fn write_frame(&mut self, nv12: &[u8], ts: u64) {
            unsafe {
                let h = self.base as *mut QueueHeader;
                let inc = std::ptr::read_volatile(&(*h).write_idx).wrapping_add(1);
                std::ptr::write_volatile(&mut (*h).write_idx, inc);
                let off = self.offsets[(inc % 3) as usize] as usize;
                std::ptr::write_volatile(self.base.add(off) as *mut u64, ts);
                let n = nv12.len().min(self.frame_size);
                std::ptr::copy_nonoverlapping(
                    nv12.as_ptr(),
                    self.base.add(off + FRAME_HEADER_SIZE),
                    n,
                );
                fence(Ordering::Release);
                std::ptr::write_volatile(&mut (*h).read_idx, inc);
                std::ptr::write_volatile(&mut (*h).state, STATE_READY);
            }
        }
    }

    impl Drop for ObsQueue {
        fn drop(&mut self) {
            unsafe {
                std::ptr::write_volatile(
                    &mut (*(self.base as *mut QueueHeader)).state,
                    STATE_STOPPING,
                );
                sys::UnmapViewOfFile(self.base as *const _);
                sys::CloseHandle(self.handle);
            }
        }
    }

    /// OBS records its last virtual-camera format in %APPDATA%\obs-virtualcam.txt ("WxHxINTERVAL");
    /// the camera filter offers that format to apps that open it before the queue exists.
    pub fn remember_format(cx: u32, cy: u32, interval: u64) {
        if let Ok(dir) = std::env::var("APPDATA") {
            let _ = std::fs::write(
                std::path::Path::new(&dir).join("obs-virtualcam.txt"),
                format!("{cx}x{cy}x{interval}"),
            );
        }
    }
}

/// The five kernel32 calls the queue needs. On Windows they're the real thing; elsewhere an
/// in-process stand-in with the same signatures, so the queue code compiles and is unit-tested on
/// every platform.
#[allow(non_snake_case)]
mod sys {
    use std::ffi::c_void;
    pub type Handle = isize;
    pub const INVALID_HANDLE_VALUE: Handle = -1;
    pub const PAGE_READWRITE: u32 = 0x04;
    pub const FILE_MAP_READ: u32 = 0x0004;
    pub const FILE_MAP_ALL_ACCESS: u32 = 0x000F_001F;

    #[cfg(windows)]
    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn OpenFileMappingW(desired_access: u32, inherit: i32, name: *const u16) -> Handle;
        pub fn CreateFileMappingW(
            file: Handle,
            attrs: *const c_void,
            protect: u32,
            size_high: u32,
            size_low: u32,
            name: *const u16,
        ) -> Handle;
        pub fn MapViewOfFile(
            mapping: Handle,
            access: u32,
            off_high: u32,
            off_low: u32,
            bytes: usize,
        ) -> *mut c_void;
        pub fn UnmapViewOfFile(addr: *const c_void) -> i32;
        pub fn CloseHandle(h: Handle) -> i32;
    }

    #[cfg(not(windows))]
    pub use mock::*;

    #[cfg(not(windows))]
    mod mock {
        use super::{Handle, c_void};
        use std::sync::Mutex;

        struct Mapping {
            name: String,
            mem: Box<[u64]>, // u64 so the base is 8-aligned like a real view
            refs: usize,
        }
        static MAPS: Mutex<Vec<Option<Mapping>>> = Mutex::new(Vec::new());

        unsafe fn name_of(p: *const u16) -> String {
            let mut v = vec![];
            let mut i = 0;
            unsafe {
                while *p.add(i) != 0 {
                    v.push(*p.add(i));
                    i += 1;
                }
            }
            String::from_utf16_lossy(&v)
        }
        fn find(maps: &[Option<Mapping>], name: &str) -> Option<usize> {
            maps.iter()
                .position(|m| m.as_ref().is_some_and(|m| m.name == name))
        }
        pub unsafe fn OpenFileMappingW(_a: u32, _i: i32, name: *const u16) -> Handle {
            let n = unsafe { name_of(name) };
            let mut maps = MAPS.lock().unwrap();
            match find(&maps, &n) {
                Some(i) => {
                    maps[i].as_mut().unwrap().refs += 1;
                    i as Handle + 1
                }
                None => 0,
            }
        }
        pub unsafe fn CreateFileMappingW(
            _f: Handle,
            _a: *const c_void,
            _p: u32,
            _hi: u32,
            lo: u32,
            name: *const u16,
        ) -> Handle {
            let n = unsafe { name_of(name) };
            let mut maps = MAPS.lock().unwrap();
            if let Some(i) = find(&maps, &n) {
                maps[i].as_mut().unwrap().refs += 1;
                return i as Handle + 1;
            }
            maps.push(Some(Mapping {
                name: n,
                mem: vec![0u64; (lo as usize).div_ceil(8)].into_boxed_slice(),
                refs: 1,
            }));
            maps.len() as Handle
        }
        pub unsafe fn MapViewOfFile(
            h: Handle,
            _a: u32,
            _hi: u32,
            _lo: u32,
            _b: usize,
        ) -> *mut c_void {
            let mut maps = MAPS.lock().unwrap();
            maps.get_mut(h as usize - 1)
                .and_then(|m| m.as_mut())
                .map(|m| m.mem.as_mut_ptr() as *mut c_void)
                .unwrap_or(std::ptr::null_mut())
        }
        pub unsafe fn UnmapViewOfFile(_p: *const c_void) -> i32 {
            1
        }
        pub unsafe fn CloseHandle(h: Handle) -> i32 {
            let mut maps = MAPS.lock().unwrap();
            if let Some(slot) = maps.get_mut(h as usize - 1) {
                if let Some(m) = slot.as_mut() {
                    m.refs -= 1;
                    if m.refs == 0 {
                        *slot = None;
                    }
                }
            }
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::obs_queue::*;
    use super::*;

    #[test]
    fn header_matches_obs_c_layout() {
        // C: 3 x u32, u32[3], 3 x u32, 4 bytes padding, u64, u32[8] = 80 bytes
        assert_eq!(std::mem::size_of::<QueueHeader>(), 80);
        assert_eq!(std::mem::offset_of!(QueueHeader, offsets), 12);
        assert_eq!(std::mem::offset_of!(QueueHeader, cx), 28);
        assert_eq!(std::mem::offset_of!(QueueHeader, interval), 40);
        let (offs, size) = layout(1280, 720);
        let frame = 1280 * 720 * 3 / 2;
        assert_eq!(offs[0], 96);
        assert_eq!(offs[1] as usize, (96 + frame + 32 + 31) & !31);
        assert_eq!(size % 32, 0);
    }

    #[test]
    fn colours_convert_to_studio_range() {
        let px = |r: u8, g: u8, b: u8| {
            let rgba: Vec<u8> = (0..4).flat_map(|_| [r, g, b, 255]).collect();
            let mut out = vec![0u8; 6];
            rgba_to_nv12(&rgba, 2, 2, &mut out);
            (out[0], out[4], out[5])
        };
        assert_eq!(px(255, 255, 255), (235, 128, 128));
        assert_eq!(px(0, 0, 0), (16, 128, 128));
        let (y, u, v) = px(255, 0, 0);
        assert!(
            (81..=83).contains(&y) && (89..=91).contains(&u) && (239..=241).contains(&v),
            "{y} {u} {v}"
        );
    }

    /// Port of OBS's reader (video_queue_open / video_queue_state / video_queue_read) against our writer.
    #[test]
    fn obs_reader_sees_our_frames() {
        let (cx, cy) = (64u32, 32u32);
        let interval = 10_000_000 / 60;
        let mut q = ObsQueue::create(cx, cy, interval).unwrap();
        assert!(
            ObsQueue::create(cx, cy, interval).is_err(),
            "second writer must be refused"
        );
        let name = wide(NAME);
        unsafe {
            let h = sys::OpenFileMappingW(sys::FILE_MAP_READ, 0, name.as_ptr());
            assert_ne!(h, 0);
            let base = sys::MapViewOfFile(h, sys::FILE_MAP_READ, 0, 0, 0) as *const u8;
            let hdr = &*(base as *const QueueHeader);
            assert_eq!(hdr.state, STATE_STARTING);
            assert_eq!((hdr.cx, hdr.cy, hdr.interval), (cx, cy, interval));
            for n in 1..=5u8 {
                let frame: Vec<u8> = (0..(cx * cy * 3 / 2) as usize)
                    .map(|i| (i as u8).wrapping_add(n))
                    .collect();
                q.write_frame(&frame, n as u64 * 1000);
                assert_eq!(hdr.state, STATE_READY);
                let inc = hdr.read_idx;
                let off = hdr.offsets[(inc % 3) as usize] as usize;
                let ts = *(base.add(off) as *const u64);
                let got =
                    std::slice::from_raw_parts(base.add(off + FRAME_HEADER_SIZE), frame.len());
                assert_eq!(ts, n as u64 * 1000);
                assert_eq!(got, &frame[..]);
            }
            drop(q);
            assert_eq!(hdr.state, STATE_STOPPING);
            sys::CloseHandle(h);
        }
    }
}
