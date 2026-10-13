//! Push-to-talk on F8. A global hotkey listener thread flips a shared gate
//! consulted by the microphone worker: while the key is up, microphone
//! samples never reach the engine.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use global_hotkey::hotkey::{Code, HotKey};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

const POLL: Duration = Duration::from_millis(50);

/// Shared between the hotkey thread and the microphone worker.
pub struct Ptt {
    held: AtomicBool,
}

impl Ptt {
    pub fn is_held(&self) -> bool {
        self.held.load(Ordering::Relaxed)
    }

    /// Flip the gate; returns true when the state actually changed, so the
    /// status is printed once per transition.
    pub fn set_held(&self, held: bool) -> bool {
        self.held.swap(held, Ordering::Relaxed) != held
    }
}

/// Whether samples may enter the engine: `None` (always-open channel) or a
/// held PTT gate.
pub fn admits(gate: Option<&Arc<Ptt>>) -> bool {
    gate.is_none_or(|ptt| ptt.is_held())
}

/// True exactly on the released edge (held on the previous check, up now);
/// the microphone worker flushes the channel tail then.
pub fn released_edge(was_held: bool, held: bool) -> bool {
    was_held && !held
}

/// Keeps the hotkey registered for as long as it is alive (dropping the
/// manager unregisters it); the gate is the live pressed/released state.
pub struct PttHandle {
    gate: Arc<Ptt>,
    _manager: GlobalHotKeyManager,
}

impl PttHandle {
    pub fn gate(&self) -> &Arc<Ptt> {
        &self.gate
    }
}

/// Register F8 and spawn the listener thread. The returned gate starts
/// released; press/release transitions print `[ptt] held`/`[ptt] released`.
pub fn start() -> Result<PttHandle> {
    let manager = GlobalHotKeyManager::new().context("cannot create the global hotkey manager")?;
    let hotkey = HotKey::new(None, Code::F8);
    manager
        .register(hotkey)
        .context("cannot register F8 for push-to-talk (is it taken by another app?)")?;
    let gate = Arc::new(Ptt {
        held: AtomicBool::new(false),
    });
    let listener_gate = gate.clone();
    thread::Builder::new()
        .name("ptt-f8".to_string())
        .spawn(move || listen(hotkey.id(), listener_gate))
        .context("failed to spawn the PTT thread")?;
    Ok(PttHandle {
        gate,
        _manager: manager,
    })
}

fn listen(id: u32, gate: Arc<Ptt>) {
    let receiver = GlobalHotKeyEvent::receiver();
    loop {
        if let Ok(event) = receiver.recv_timeout(POLL) {
            if event.id != id {
                continue;
            }
            let held = matches!(event.state, HotKeyState::Pressed);
            if gate.set_held(held) {
                if held {
                    println!("[ptt] held");
                } else {
                    println!("[ptt] released");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate() -> Arc<Ptt> {
        Arc::new(Ptt {
            held: AtomicBool::new(false),
        })
    }

    #[test]
    fn samples_are_blocked_while_released() {
        let ptt = gate();
        assert!(!ptt.is_held());
        assert!(!admits(Some(&ptt)));
        assert!(ptt.set_held(true));
        assert!(admits(Some(&ptt)));
        assert!(ptt.set_held(false));
        assert!(!admits(Some(&ptt)));
    }

    #[test]
    fn channel_without_ptt_is_always_open() {
        assert!(admits(None));
    }

    #[test]
    fn set_held_reports_only_real_transitions() {
        let ptt = gate();
        assert!(ptt.set_held(true));
        assert!(!ptt.set_held(true));
        assert!(ptt.set_held(false));
        assert!(!ptt.set_held(false));
    }

    #[test]
    fn release_edge_triggers_only_on_the_falling_transition() {
        assert!(!released_edge(false, false));
        assert!(!released_edge(false, true));
        assert!(!released_edge(true, true));
        assert!(released_edge(true, false));
    }
}
