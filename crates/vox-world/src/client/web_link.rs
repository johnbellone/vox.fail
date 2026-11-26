//! The link in the browser: the page's own WebSocket. Its callbacks fill the same inbox the
//! desktop's thread does; what goes out is sent each frame by [`pump`].

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::{BinaryType, CloseEvent, MessageEvent, WebSocket};

use super::{Incoming, ToClient, wire};

struct Open {
    ws: Option<WebSocket>,
    url: String,
    out: mpsc::Receiver<Vec<u8>>,
    inbox: mpsc::Sender<Incoming>,
    stop: Arc<AtomicBool>,
    bye: Rc<RefCell<bool>>,
    retry_at: Option<f64>,
    wait: f64,
}

thread_local! {
    static LINK: RefCell<Option<Open>> = const { RefCell::new(None) };
}

fn now() -> f64 {
    js_sys::Date::now() / 1000.0
}

fn connect(o: &mut Open) {
    let ws = match WebSocket::new(&o.url) {
        Ok(ws) => ws,
        Err(e) => {
            let _ = o.inbox.send(Incoming::Status(format!("Can't reach the world: {e:?}")));
            o.retry_at = Some(now() + o.wait);
            return;
        }
    };
    ws.set_binary_type(BinaryType::Arraybuffer);
    let inbox = o.inbox.clone();
    let on_open = Closure::<dyn FnMut()>::new(move || {
        let _ = inbox.send(Incoming::Status("Connected".into()));
    });
    ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    on_open.forget();
    let inbox = o.inbox.clone();
    let bye = o.bye.clone();
    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
        if let Ok(buf) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
            let b = js_sys::Uint8Array::new(&buf).to_vec();
            if matches!(wire::decode::<ToClient>(&b), Ok(ToClient::Bye { .. })) {
                *bye.borrow_mut() = true;
            }
            let _ = inbox.send(Incoming::Data(b));
        }
    });
    ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    on_message.forget();
    let inbox = o.inbox.clone();
    let on_close = Closure::<dyn FnMut(CloseEvent)>::new(move |_e: CloseEvent| {
        let _ = inbox.send(Incoming::Status("Reconnecting".into()));
    });
    ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
    on_close.forget();
    o.ws = Some(ws);
}

pub fn open(url: String, out: mpsc::Receiver<Vec<u8>>, inbox: mpsc::Sender<Incoming>, stop: Arc<AtomicBool>) {
    LINK.with(|l| {
        if let Some(old) = l.borrow_mut().take() {
            if let Some(ws) = old.ws {
                let _ = ws.close();
            }
        }
        let mut o = Open { ws: None, url, out, inbox, stop, bye: Rc::new(RefCell::new(false)), retry_at: None, wait: 0.5 };
        connect(&mut o);
        *l.borrow_mut() = Some(o);
    });
}

/// Send what's queued; reconnect a dropped link (unless the server said goodbye).
pub fn pump() {
    LINK.with(|l| {
        let mut l = l.borrow_mut();
        let Some(o) = l.as_mut() else { return };
        if o.stop.load(Ordering::Relaxed) {
            if let Some(ws) = o.ws.take() {
                let _ = ws.close();
            }
            *l = None;
            return;
        }
        let state = o.ws.as_ref().map(|ws| ws.ready_state());
        match state {
            Some(WebSocket::OPEN) => {
                let ws = o.ws.as_ref().unwrap();
                while let Ok(b) = o.out.try_recv() {
                    let _ = ws.send_with_u8_array(&b);
                }
            }
            Some(WebSocket::CONNECTING) => {
                // anything queued before connecting is stale: the hello goes first
                while o.out.try_recv().is_ok() {}
            }
            _ => {
                if *o.bye.borrow() {
                    return;
                }
                match o.retry_at {
                    None => o.retry_at = Some(now() + o.wait),
                    Some(t) if now() >= t => {
                        o.retry_at = None;
                        o.wait = (o.wait * 2.0).min(10.0);
                        connect(o);
                    }
                    _ => {}
                }
            }
        }
    });
}
