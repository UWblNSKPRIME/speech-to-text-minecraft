//! TCP bridge to the Fabric mod. One background thread owns the socket,
//! retries the connection every 2 s and discards stale lines while the mod
//! is away; recognition workers never block on it.

use std::io::Write;
use std::net::TcpStream;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
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

/// Chat display name for each audio channel (`me` / `discord`).
#[derive(Clone)]
pub struct SpeakerNames {
    pub me: String,
    pub discord: String,
}

/// Prefix a transcript for the mod: `<name>: <text>`. Slash commands are
/// sent raw so the mod's guard fires; empty input yields an empty string
/// (nothing is sent). The console log keeps the un-prefixed text.
pub fn outgoing_text(name: &str, text: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        String::new()
    } else if text.starts_with('/') {
        text.to_string()
    } else {
        format!("{name}: {text}")
    }
}

/// Cheap sending handle: clone it and pass one copy per channel worker.
#[derive(Clone)]
pub struct ModBridge {
    tx: SyncSender<String>,
    names: SpeakerNames,
}

impl ModBridge {
    /// Spawn the bridge thread (daemon) and return a sending handle.
    pub fn start(names: SpeakerNames) -> Self {
        let (tx, rx) = sync_channel(QUEUE_CAP);
        let thread_bridge = Self {
            tx: tx.clone(),
            names: names.clone(),
        };
        thread::Builder::new()
            .name("mod-bridge".to_string())
            .spawn(move || thread_bridge.run(rx))
            .expect("failed to spawn the mod bridge thread");
        Self { tx, names }
    }

    /// Queue one protocol line with the speaker's chat name prefixed into the
    /// text. Never blocks: drops the line when the queue is full (the console
    /// output remains the source of truth).
    pub fn send(&self, kind: &str, speaker: &str, text: &str) {
        let name = if speaker == "me" {
            &self.names.me
        } else {
            &self.names.discord
        };
        let text = outgoing_text(name, text);
        if text.is_empty() {
            return;
        }
        let line = serde_json::to_string(&TranscriptLine {
            t: kind,
            speaker,
            text: &text,
        })
        .expect("a transcript of &str always serializes");
        let _ = self.tx.try_send(line);
    }

    fn run(&self, rx: Receiver<String>) {
        // This thread owns a sender clone (`self`), so `rx` never disconnects:
        // the loop runs until the process exits.
        eprintln!(
            "[bridge] waiting for the mod on {MOD_ADDR} (transcripts go to the console either way)"
        );
        let mut socket: Option<TcpStream> = None;
        loop {
            match socket {
                Some(ref mut stream) => {
                    if let Ok(line) = rx.recv_timeout(POLL) {
                        if let Err(err) = writeln!(stream, "{line}") {
                            eprintln!("[bridge] mod disconnected: {err}");
                            socket = None;
                        }
                    }
                }
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

    #[test]
    fn outgoing_text_prefixes_the_speaker_name() {
        assert_eq!(outgoing_text("Вэвси", "привет мир"), "Вэвси: привет мир");
        assert_eq!(outgoing_text("Аури", "привет"), "Аури: привет");
        assert_eq!(outgoing_text("Bot", "  hello  "), "Bot: hello");
    }

    #[test]
    fn outgoing_text_keeps_slash_commands_raw() {
        assert_eq!(outgoing_text("Вэвси", "/kill @e"), "/kill @e");
        assert_eq!(outgoing_text("Аури", "  /spawn pig "), "/spawn pig");
    }

    #[test]
    fn outgoing_text_empty_stays_empty() {
        assert_eq!(outgoing_text("Вэвси", ""), "");
        assert_eq!(outgoing_text("Аури", "  \n "), "");
    }

    #[test]
    fn send_prefixes_and_skips_empty() {
        let (tx, rx) = sync_channel(4);
        let bridge = ModBridge {
            tx,
            names: SpeakerNames {
                me: "Вэвси".to_string(),
                discord: "Аури".to_string(),
            },
        };

        bridge.send("draft", "me", "   ");
        assert!(rx.try_recv().is_err(), "empty text must not be queued");

        bridge.send("final", "discord", "привет");
        assert_eq!(
            rx.try_recv().expect("one line queued"),
            r#"{"t":"final","speaker":"discord","text":"Аури: привет"}"#
        );

        bridge.send("final", "me", "/spawn pig");
        assert_eq!(
            rx.try_recv().expect("one line queued"),
            r#"{"t":"final","speaker":"me","text":"/spawn pig"}"#
        );
    }
}
