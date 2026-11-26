//! vox-link: a WebSocket kept open on its own thread.
//!
//! [`spawn`] starts a thread that connects, sends what the app queues, hands back what
//! arrives, and reconnects with backoff (half a second, doubling to ten) when the connection
//! drops. What differs between uses is a [`Dial`]: where to connect (the relay's host first
//! registers its show), what to say when the far end refuses, whether a message means
//! goodbye (don't reconnect), and whether messages queued while disconnected are stale.
//!
//! Desktop only: in the browser the page's own WebSocket does this (vox-world's `web_link`).

#![cfg(not(target_arch = "wasm32"))]

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

/// What the link hands back.
#[derive(Debug)]
pub enum Event<T> {
    /// "Connected", "Reconnecting", or why it couldn't connect
    Status(String),
    /// a binary message, with when it arrived
    Data(Vec<u8>, Instant),
    /// a text message
    Text(String),
    /// something the [`Dial`] wanted to tell the app (the relay's new show, say)
    App(T),
}

/// How one kind of link connects.
pub trait Dial<T>: Send + 'static {
    /// The address to connect to now. Work done first (registering a show) can report to the
    /// app through `tell`. An error is shown as the status and tried again after a wait.
    fn address(&mut self, tell: &dyn Fn(T)) -> Result<String, String>;
    /// Connecting failed: `status` is the HTTP status the server answered with (0 for none).
    /// Returns what to show.
    fn refused(&mut self, status: u16, error: &str) -> String {
        let _ = status;
        format!("Can't connect: {error}")
    }
    /// True if this message means the far end is done with us: close and don't come back.
    fn goodbye(&self, _message: &[u8]) -> bool {
        false
    }
    /// True to throw away what was queued while disconnected (a hello must go first).
    fn drop_stale(&self) -> bool {
        false
    }
}

/// The app's end of a link: queue messages out, take events in. Dropping it stops the thread.
pub struct Link<T> {
    out: mpsc::Sender<Vec<u8>>,
    inbox: Mutex<mpsc::Receiver<Event<T>>>,
    stop: Arc<AtomicBool>,
}

impl<T> Link<T> {
    /// Queue a message (sent as soon as connected).
    pub fn send(&self, bytes: Vec<u8>) {
        let _ = self.out.send(bytes);
    }
    /// A sender for messages, for another thread to queue on (the voice encoder, say).
    pub fn sender(&self) -> mpsc::Sender<Vec<u8>> {
        self.out.clone()
    }
    /// Everything that has arrived since last time.
    pub fn drain(&self) -> Vec<Event<T>> {
        let inbox = self.inbox.lock().unwrap_or_else(|e| e.into_inner());
        inbox.try_iter().collect()
    }
}

impl<T> Drop for Link<T> {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Start a link on a thread called `name`.
pub fn spawn<T: Send + 'static>(name: &str, dial: impl Dial<T>) -> Link<T> {
    let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>();
    let (in_tx, in_rx) = mpsc::channel::<Event<T>>();
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    std::thread::Builder::new().name(name.into()).spawn(move || run(dial, out_rx, in_tx, stop2)).expect("link thread");
    Link { out: out_tx, inbox: Mutex::new(in_rx), stop }
}

const FIRST_WAIT: Duration = Duration::from_millis(500);
const LONGEST_WAIT: Duration = Duration::from_secs(10);

fn run<T: Send + 'static>(mut dial: impl Dial<T>, out: mpsc::Receiver<Vec<u8>>, inbox: mpsc::Sender<Event<T>>, stop: Arc<AtomicBool>) {
    let mut wait = FIRST_WAIT;
    let back_off = |wait: &mut Duration| {
        std::thread::sleep(*wait);
        *wait = (*wait * 2).min(LONGEST_WAIT);
    };
    while !stop.load(Ordering::Relaxed) {
        let tell = |t: T| {
            let _ = inbox.send(Event::App(t));
        };
        let url = match dial.address(&tell) {
            Ok(u) => u,
            Err(e) => {
                let _ = inbox.send(Event::Status(e));
                back_off(&mut wait);
                continue;
            }
        };
        let mut sock = match tungstenite::connect(&url) {
            Ok((s, _)) => s,
            Err(e) => {
                let status = match &e {
                    tungstenite::Error::Http(r) => r.status().as_u16(),
                    _ => 0,
                };
                let _ = inbox.send(Event::Status(dial.refused(status, &e.to_string())));
                back_off(&mut wait);
                continue;
            }
        };
        wait = FIRST_WAIT;
        // a short read timeout keeps sends flowing between reads
        let short = Some(Duration::from_millis(5));
        match sock.get_mut() {
            tungstenite::stream::MaybeTlsStream::Plain(s) => {
                let _ = s.set_read_timeout(short);
                let _ = s.set_nodelay(true);
            }
            tungstenite::stream::MaybeTlsStream::Rustls(s) => {
                let _ = s.get_mut().set_read_timeout(short);
                let _ = s.get_mut().set_nodelay(true);
            }
            _ => {}
        }
        if dial.drop_stale() {
            while out.try_recv().is_ok() {}
        }
        let _ = inbox.send(Event::Status("Connected".into()));
        let mut bye = false;
        'live: while !stop.load(Ordering::Relaxed) {
            while let Ok(bytes) = out.try_recv() {
                if sock.send(tungstenite::Message::binary(bytes)).is_err() {
                    break 'live;
                }
            }
            loop {
                match sock.read() {
                    Ok(tungstenite::Message::Binary(b)) => {
                        bye |= dial.goodbye(&b);
                        let _ = inbox.send(Event::Data(b.to_vec(), Instant::now()));
                    }
                    Ok(tungstenite::Message::Text(t)) => {
                        let _ = inbox.send(Event::Text(t.as_str().to_string()));
                    }
                    Ok(tungstenite::Message::Close(_)) => break 'live,
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => break,
                    Err(_) => break 'live,
                }
            }
        }
        let _ = sock.close(None);
        if bye {
            return; // the far end said why; trying again won't help
        }
        if !stop.load(Ordering::Relaxed) {
            let _ = inbox.send(Event::Status("Reconnecting".into()));
            std::thread::sleep(wait);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    struct To(String);
    impl Dial<()> for To {
        fn address(&mut self, _: &dyn Fn(())) -> Result<String, String> {
            Ok(self.0.clone())
        }
        fn goodbye(&self, m: &[u8]) -> bool {
            m == b"bye"
        }
    }

    fn wait_for(link: &Link<()>, mut want: impl FnMut(&Event<()>) -> bool) -> Vec<Event<()>> {
        let mut seen = vec![];
        for _ in 0..400 {
            for e in link.drain() {
                let hit = want(&e);
                seen.push(e);
                if hit {
                    return seen;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("never came: {seen:?}");
    }

    #[test]
    fn sends_receives_and_stops_after_goodbye() {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = server.local_addr().unwrap();
        let echo = std::thread::spawn(move || {
            let (stream, _) = server.accept().unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            let m = ws.read().unwrap();
            ws.send(m).unwrap();
            ws.send(tungstenite::Message::binary(b"bye".to_vec())).unwrap();
            let _ = ws.close(None);
            // a second connection would mean the link came back after goodbye
            server.set_nonblocking(true).unwrap();
            std::thread::sleep(Duration::from_millis(1500));
            server.accept().is_err()
        });
        let link = spawn("test-link", To(format!("ws://{addr}/")));
        link.send(b"hello".to_vec());
        wait_for(&link, |e| matches!(e, Event::Data(b, _) if b == b"hello"));
        wait_for(&link, |e| matches!(e, Event::Data(b, _) if b == b"bye"));
        assert!(echo.join().unwrap(), "it didn't reconnect after goodbye");
    }

    #[test]
    fn refusals_are_reported_and_retried() {
        // nothing listens here
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let link = spawn("test-refused", To(format!("ws://127.0.0.1:{port}/")));
        let seen = wait_for(&link, |e| matches!(e, Event::Status(s) if s.starts_with("Can't connect")));
        assert!(!seen.is_empty());
    }
}
