//! `PortBroker` — the single owner of every serial port lease (`ARCHITECTURE.md` §3
//! `core/device`, `FR-DEV-4`).
//!
//! This tracks *who currently holds* each port, synchronously and in memory — it does
//! **not** itself drive the "stop monitor → upload → restart monitor" sequence FR-DEV-4
//! describes. That sequence needs to actually stop a running monitor task and later start
//! a new one with the same settings, which means touching `commands::monitor`'s Tauri
//! state and spawning async work — not something a `Lease`'s synchronous `Drop` can do.
//! So the broker gives `preempt: true` callers a lease unconditionally (trusting they will
//! actually stop the previous holder first), and the *orchestration* of "stop, do the
//! thing, restart" lives one layer up, in `commands::pipeline::run_build_or_upload` and
//! `commands::monitor` — see `SPEC.md` §8 open question 22.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseHolder {
    Monitor,
    Upload,
    Telemetry,
    External,
}

type Ports = Arc<Mutex<HashMap<String, LeaseHolder>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortBusy {
    pub held_by: LeaseHolder,
}

/// An RAII guard: dropping it releases the port. Global (not per-window) — one `PortBroker`
/// instance is Tauri-managed state shared across every window (`FR-UI-9`).
#[derive(Debug)]
pub struct Lease {
    port: String,
    ports: Ports,
}

impl Lease {
    pub fn port(&self) -> &str {
        &self.port
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if let Ok(mut map) = self.ports.lock().or_else(|e| Ok::<_, ()>(e.into_inner())) {
            map.remove(&self.port);
        }
    }
}

#[derive(Default, Clone)]
pub struct PortBroker {
    ports: Ports,
}

impl PortBroker {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, LeaseHolder>> {
        // A poisoned lock (some previous holder panicked mid-access) shouldn't permanently
        // wedge port leasing for the rest of the app's lifetime — recover the data rather
        // than propagating the poison.
        self.ports.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Acquires `port` for `who`. If already held and `preempt` is `false`, returns
    /// `Err(PortBusy)` naming the current holder. If `preempt` is `true`, grants the lease
    /// regardless — the caller is responsible for having already stopped whatever held it.
    pub fn acquire(&self, port: &str, who: LeaseHolder, preempt: bool) -> Result<Lease, PortBusy> {
        let mut map = self.lock();
        if let Some(&existing) = map.get(port) {
            if !preempt {
                return Err(PortBusy { held_by: existing });
            }
        }
        map.insert(port.to_string(), who);
        drop(map);
        Ok(Lease {
            port: port.to_string(),
            ports: self.ports.clone(),
        })
    }

    pub fn current_holder(&self, port: &str) -> Option<LeaseHolder> {
        self.lock().get(port).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unheld_port_is_free_to_acquire() {
        let broker = PortBroker::new();
        assert_eq!(broker.current_holder("COM3"), None);
        let lease = broker.acquire("COM3", LeaseHolder::Monitor, false).expect("acquire");
        assert_eq!(broker.current_holder("COM3"), Some(LeaseHolder::Monitor));
        drop(lease);
        assert_eq!(broker.current_holder("COM3"), None);
    }

    #[test]
    fn a_held_port_refuses_a_second_lease_without_preempt() {
        let broker = PortBroker::new();
        let _lease = broker.acquire("COM3", LeaseHolder::Monitor, false).expect("acquire");
        let err = broker.acquire("COM3", LeaseHolder::Upload, false).unwrap_err();
        assert_eq!(err.held_by, LeaseHolder::Monitor);
    }

    #[test]
    fn preempt_grants_the_lease_regardless_of_the_current_holder() {
        let broker = PortBroker::new();
        let _lease = broker.acquire("COM3", LeaseHolder::Monitor, false).expect("acquire");
        let upload_lease = broker.acquire("COM3", LeaseHolder::Upload, true).expect("preempt");
        assert_eq!(broker.current_holder("COM3"), Some(LeaseHolder::Upload));
        drop(upload_lease);
        assert_eq!(broker.current_holder("COM3"), None);
    }

    #[test]
    fn different_ports_are_independent() {
        let broker = PortBroker::new();
        let _a = broker.acquire("COM3", LeaseHolder::Monitor, false).expect("acquire COM3");
        let _b = broker.acquire("COM4", LeaseHolder::Upload, false).expect("acquire COM4");
        assert_eq!(broker.current_holder("COM3"), Some(LeaseHolder::Monitor));
        assert_eq!(broker.current_holder("COM4"), Some(LeaseHolder::Upload));
    }

    #[test]
    fn dropping_one_lease_does_not_affect_a_lease_on_another_port() {
        let broker = PortBroker::new();
        let a = broker.acquire("COM3", LeaseHolder::Monitor, false).expect("acquire COM3");
        let _b = broker.acquire("COM4", LeaseHolder::Upload, false).expect("acquire COM4");
        drop(a);
        assert_eq!(broker.current_holder("COM3"), None);
        assert_eq!(broker.current_holder("COM4"), Some(LeaseHolder::Upload));
    }
}
