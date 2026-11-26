//! The Twitch reader: an anonymous IRC connection on its own thread.

#[allow(unused_imports)]
use super::*;

/// Reads one Twitch channel anonymously, reconnecting if the connection drops.
pub fn start_twitch(
    host: String,
    port: u16,
    channel: String,
    status: Arc<Mutex<String>>,
) -> Receiver<ChatEvent> {
    let (tx, rx) = mpsc::channel();
    let channel = channel.trim().trim_start_matches('#').to_ascii_lowercase();
    std::thread::Builder::new()
        .name("twitch".into())
        .spawn(move || loop {
            let addr = (host.as_str(), port).to_socket_addrs().ok().and_then(|mut a| a.next());
            let conn = addr.and_then(|a| TcpStream::connect_timeout(&a, Duration::from_secs(8)).ok());
            let Some(stream) = conn else {
                *status.lock().unwrap_or_else(|e| e.into_inner()) = format!("Twitch: can't reach chat, retrying ({channel})");
                std::thread::sleep(Duration::from_secs(5));
                continue;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(360)));
            let nick = format!("justinfan{}", 10000 + (std::process::id() % 80000));
            let mut w = &stream;
            // twitch.tv/commands + tags: also hear about bans and timeouts (CLEARCHAT)
            if w.write_all(format!("CAP REQ :twitch.tv/commands twitch.tv/tags\r\nPASS SCHMOOPIIE\r\nNICK {nick}\r\nJOIN #{channel}\r\n").as_bytes()).is_err() {
                continue;
            }
            *status.lock().unwrap_or_else(|e| e.into_inner()) = format!("Twitch: reading #{channel}");
            let mut r = BufReader::new(&stream);
            let mut line = String::new();
            loop {
                line.clear();
                match r.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => match parse_irc(&line) {
                        IrcLine::Ping(p) => {
                            let _ = (&stream).write_all(format!("PONG {p}\r\n").as_bytes());
                        }
                        IrcLine::Msg(m) => {
                            if tx.send(ChatEvent::Msg(m)).is_err() {
                                return; // app closed
                            }
                        }
                        IrcLine::Ban(u, secs) => {
                            if tx.send(ChatEvent::Ban(u, secs)).is_err() {
                                return;
                            }
                        }
                        IrcLine::Other => {
                            if let Some(ev) = parse_notice(&line) {
                                if tx.send(ev).is_err() {
                                    return;
                                }
                            }
                        }
                    },
                }
            }
            *status.lock().unwrap_or_else(|e| e.into_inner()) = format!("Twitch: disconnected from #{channel}, reconnecting");
            std::thread::sleep(Duration::from_secs(3));
        })
        .expect("spawn twitch thread");
    rx
}
