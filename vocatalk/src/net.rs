//! TCP bridge to the Fabric mod. One background thread owns the socket,
//! retries the connection every 2 s and discards stale lines while the mod
//! is away; recognition workers never block on it.

use std::io::Write;
use std::net::TcpStream;
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender};
use std::thread;
use std::time::Duration;

use serde::Serialize;

const MOD_ADDR: &str = "127.0.0.1:47832";
const CONNECT_RETRY: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(200);
/// Cap on queued lines so a long disconnect never blocks the workers.
const QUEUE_CAP: usize = 100;

/// Wire format of the mod's `Transcript` (JSON-lines, one object per line).
#[derive(Serialize)]
struct TranscriptLine<'a> {
    t: &'a str,
    speaker: &'a str,
    text: &'a str,
}

/// Cheap sending handle: clone it and pass one copy per channel worker.
#[derive(Clone)]
pub struct ModBridge {
    tx: SyncSender<String>,
}

impl ModBridge {
    /// Spawn the bridge thread (daemon) and return a sending handle.
    pub fn start() -> Self {
        let (tx, rx) = sync_channel(QUEUE_CAP);
        let thread_bridge = Self { tx: tx.clone() };
        thread::Builder::new()
            .name("mod-bridge".to_string())
            .spawn(move || thread_bridge.run(rx))
            .expect("failed to spawn the mod bridge thread");
        Self { tx }
    }

    /// Queue one protocol line. Never blocks: drops the line when the queue
    /// is full (the console output remains the source of truth).
    pub fn send(&self, kind: &str, speaker: &str, text: &str) {
        let line = serde_json::to_string(&TranscriptLine {
            t: kind,
            speaker,
            text,
        })
        .expect("a transcript of &str always serializes");
        let _ = self.tx.try_send(line);
    }

    fn run(&self, rx: Receiver<String>) {
        eprintln!(
            "[bridge] waiting for the mod on {MOD_ADDR} (transcripts go to the console either way)"
        );
        let mut socket: Option<TcpStream> = None;
        loop {
            match socket {
                Some(ref mut stream) => match rx.recv_timeout(POLL) {
                    Ok(line) => {
                        if let Err(err) = writeln!(stream, "{line}") {
                            eprintln!("[bridge] mod disconnected: {err}");
                            socket = None;
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                },
                None => {
                    // Lines queued while the mod was away are stale: drop them
                    // and retry the connection at most every CONNECT_RETRY.
                    while rx.try_recv().is_ok() {}
                    thread::sleep(CONNECT_RETRY);
                    if let Ok(stream) = TcpStream::connect(MOD_ADDR) {
                        let _ = stream.set_nodelay(true);
                        eprintln!("[bridge] connected to the mod at {MOD_ADDR}");
                        socket = Some(stream);
                    }
                    if let Err(RecvTimeoutError::Disconnected) = rx.recv_timeout(Duration::ZERO) {
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: &str, speaker: &str, text: &str) -> String {
        serde_json::to_string(&TranscriptLine {
            t: kind,
            speaker,
            text,
        })
        .expect("serializes")
    }

    #[test]
    fn protocol_line_matches_the_mod_transcript() {
        assert_eq!(
            line("final", "discord", "hello"),
            r#"{"t":"final","speaker":"discord","text":"hello"}"#
        );
        assert_eq!(
            line("draft", "me", "privet"),
            r#"{"t":"draft","speaker":"me","text":"privet"}"#
        );
    }

    #[test]
    fn protocol_line_escapes_quotes_and_newlines() {
        assert_eq!(
            line("draft", "me", "say \"hi\"\nnow"),
            r#"{"t":"draft","speaker":"me","text":"say \"hi\"\nnow"}"#
        );
    }
}
