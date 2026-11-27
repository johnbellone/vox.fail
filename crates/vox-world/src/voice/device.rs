//! Opening an audio device without stopping the game: each device runs on its own thread,
//! which opens the stream, says how that went through a [`Slot`], then holds the stream until
//! told to stop. The game polls the slot each frame instead of waiting, so turning voice on
//! never freezes a frame (opening a device can take a noticeable moment). Native only.
//!
//! The stream is made, used and dropped on that one thread, so nothing has to pretend a cpal
//! stream is `Send` (it isn't on every platform).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Where a device thread is.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// still opening
    Starting,
    /// open; the device's name
    Ready(String),
    /// couldn't open; why, in words for the player
    Failed(String),
}

/// What a device thread reports: nothing yet, then how opening went.
#[derive(Clone, Default)]
pub struct Slot(Arc<Mutex<Option<Result<String, String>>>>);

impl Slot {
    pub fn set(&self, r: Result<String, String>) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
    }

    pub fn status(&self) -> Status {
        match &*self.0.lock().unwrap_or_else(|e| e.into_inner()) {
            None => Status::Starting,
            Some(Ok(name)) => Status::Ready(name.clone()),
            Some(Err(why)) => Status::Failed(why.clone()),
        }
    }
}

/// Runs `open` on a new thread named `name`: its result goes into the returned slot, and a
/// stream it made is held until `stop` is set, then dropped on that same thread.
pub fn spawn<S: 'static>(
    name: &str,
    stop: Arc<AtomicBool>,
    open: impl FnOnce() -> Result<(S, String), String> + Send + 'static,
) -> Result<Slot, String> {
    let slot = Slot::default();
    let out = slot.clone();
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || match open() {
            Ok((stream, device)) => {
                out.set(Ok(device));
                // polled, not blocked on: the stop flag is the only way out
                while !stop.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(100));
                }
                drop(stream);
            }
            Err(why) => {
                crate::note!("[voice] {why}");
                out.set(Err(why));
            }
        })
        .map_err(|e| format!("can't start the audio thread: {e}"))?;
    Ok(slot)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(slot: &Slot) -> Status {
        for _ in 0..200 {
            if slot.status() != Status::Starting {
                return slot.status();
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        slot.status()
    }

    #[test]
    fn a_device_that_opens_is_held_until_stopped() {
        // a "stream" that notes when it's dropped
        struct Held(Arc<AtomicBool>);
        impl Drop for Held {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Relaxed);
            }
        }
        let (stop, dropped) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
        let d = dropped.clone();
        let slot = spawn("test-dev", stop.clone(), move || Ok((Held(d), "Test Mic".to_string()))).unwrap();
        assert_eq!(wait(&slot), Status::Ready("Test Mic".into()));
        std::thread::sleep(Duration::from_millis(150));
        assert!(!dropped.load(Ordering::Relaxed), "held while running");
        stop.store(true, Ordering::Relaxed);
        for _ in 0..50 {
            if dropped.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(dropped.load(Ordering::Relaxed), "dropped after stop");
    }

    #[test]
    fn a_device_that_fails_says_why() {
        let slot = spawn::<()>("test-dev", Arc::new(AtomicBool::new(false)), || Err("no microphone".into())).unwrap();
        assert_eq!(wait(&slot), Status::Failed("no microphone".into()));
    }
}
