//! The in-app serial monitor (`FR-DEV-5`). Reads the port directly through the
//! `serialport` crate — never by scraping `pio device monitor` — using the *same*
//! `monitor_*` ini options the CLI itself would read (`CLI-CONTRACT.md` §3.3), so the two
//! behave identically.
//!
//! `serialport`'s I/O is synchronous, so the actual read/write loop runs on a dedicated
//! `std::thread` (see [`spawn_io_thread`]), bridged back to async code with a plain
//! `tokio::sync::mpsc::UnboundedSender` — its `send` is a sync method, so the blocking
//! thread can call it directly with no further bridging. Opening the port happens
//! *before* the thread starts (via `tokio::task::spawn_blocking` in the command handler),
//! so a bad port or a permissions error surfaces immediately as an `AppError` rather than
//! only showing up once the thread is already running.
//!
//! Not verified against real hardware in this environment — `SPEC.md` §8 open question 26.
//! The M5 real-hardware session did verify Build/Upload; the monitor's actual serial I/O
//! (baud/parity/RTS/DTR handling, line framing) should be checked against a real board
//! before this ships.

use crate::core::project::env;
use crate::error::AppError;
use serde::Serialize;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::time::Duration;
use ts_rs::TS;

/// `IPC-CONTRACT.md` §6's `MonitorEvent` — the wire shape for `monitor_start`'s `Channel`.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "type", content = "data")]
pub enum MonitorEvent {
    Opened { port: String, baud: u32 },
    /// Already decoded per `monitor_encoding`.
    Data { chunk: String, ts_ms: u64 },
    Preempted { by: String },
    Reattached { port: String },
    Closed { reason: String },
    Error { error: AppError },
}

// ---------------------------------------------------------------------------------------
// Settings — `CLI-CONTRACT.md` §3.3 / §7.4's `monitor` option group.
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorParity {
    None,
    Even,
    Odd,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SerialMonitorSettings {
    pub baud: u32,
    pub parity: MonitorParity,
    pub rts: Option<bool>,
    pub dtr: Option<bool>,
    pub eol: String,
    pub echo: bool,
    pub encoding: String,
    pub raw: bool,
}

impl Default for SerialMonitorSettings {
    /// Only `monitor_speed`'s default (9600) is a verified sample
    /// (`CLI-CONTRACT.md` §7.4); the rest follow PlatformIO/pyserial's well-known
    /// conventions but are not confirmed against a captured fixture.
    fn default() -> Self {
        Self {
            baud: 9600,
            parity: MonitorParity::None,
            rts: None,
            dtr: None,
            eol: "CRLF".into(),
            echo: false,
            encoding: "UTF-8".into(),
            raw: false,
        }
    }
}

fn parse_bool_flag(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" => Some(true),
        "0" | "false" | "no" => Some(false),
        _ => None,
    }
}

/// Builds [`SerialMonitorSettings`] from `platformio.ini`'s text for the given env, falling
/// back to `[env]` for anything not overridden
/// (`core::project::env::read_value_with_fallback`), and to
/// [`SerialMonitorSettings::default`] for anything absent from both.
pub fn settings_from_ini(ini_text: &str, env_name: &str) -> SerialMonitorSettings {
    let defaults = SerialMonitorSettings::default();
    let read = |key: &str| env::read_value_with_fallback(ini_text, env_name, key);

    let baud = read("monitor_speed").and_then(|v| v.parse().ok()).unwrap_or(defaults.baud);
    let parity = match read("monitor_parity").as_deref() {
        Some("E") | Some("e") => MonitorParity::Even,
        Some("O") | Some("o") => MonitorParity::Odd,
        // `S` (space) and `M` (mark) are valid `pio device monitor` values but the
        // `serialport` crate has no equivalent — falls back to `None` rather than erroring.
        _ => MonitorParity::None,
    };
    let rts = read("monitor_rts").and_then(|v| parse_bool_flag(&v));
    let dtr = read("monitor_dtr").and_then(|v| parse_bool_flag(&v));
    let eol = read("monitor_eol").unwrap_or(defaults.eol);
    let echo = read("monitor_echo").and_then(|v| parse_bool_flag(&v)).unwrap_or(defaults.echo);
    let encoding = read("monitor_encoding").unwrap_or(defaults.encoding);
    let raw = read("monitor_raw").and_then(|v| parse_bool_flag(&v)).unwrap_or(defaults.raw);

    SerialMonitorSettings {
        baud,
        parity,
        rts,
        dtr,
        eol,
        echo,
        encoding,
        raw,
    }
}

/// The literal bytes appended to `monitor_send`'s text per the EOL setting. Unrecognized
/// values fall back to `CRLF`, PlatformIO's own default line ending.
pub fn eol_bytes(eol: &str) -> &'static [u8] {
    match eol.to_ascii_uppercase().as_str() {
        "LF" => b"\n",
        "CR" => b"\r",
        _ => b"\r\n",
    }
}

// ---------------------------------------------------------------------------------------
// Ring buffer — `NFR-P3`: "capped by line count and total bytes." Exact figures are not
// specified anywhere in the pack (only the log pane's 50 000-line default is; `SPEC.md`
// §8 open question 27) — these are a reasonable starting point, not a verified number.
// ---------------------------------------------------------------------------------------

pub const DEFAULT_MAX_LINES: usize = 10_000;
pub const DEFAULT_MAX_BYTES: usize = 5 * 1024 * 1024;

pub struct RingBuffer {
    max_lines: usize,
    max_bytes: usize,
    buf: VecDeque<u8>,
    line_count: usize,
}

impl RingBuffer {
    pub fn new(max_lines: usize, max_bytes: usize) -> Self {
        Self {
            max_lines,
            max_bytes,
            buf: VecDeque::new(),
            line_count: 0,
        }
    }

    pub fn push(&mut self, data: &[u8]) {
        for &b in data {
            self.buf.push_back(b);
            if b == b'\n' {
                self.line_count += 1;
            }
        }
        self.enforce_caps();
    }

    fn enforce_caps(&mut self) {
        while self.buf.len() > self.max_bytes || self.line_count > self.max_lines {
            match self.buf.pop_front() {
                Some(b'\n') => self.line_count -= 1,
                Some(_) => {}
                None => break,
            }
        }
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        self.buf.iter().copied().collect()
    }

    pub fn clear(&mut self) {
        self.buf.clear();
        self.line_count = 0;
    }

    pub fn len_bytes(&self) -> usize {
        self.buf.len()
    }
}

impl Default for RingBuffer {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_LINES, DEFAULT_MAX_BYTES)
    }
}

// ---------------------------------------------------------------------------------------
// The I/O engine.
// ---------------------------------------------------------------------------------------

pub enum MonitorCommand {
    Send(Vec<u8>),
    Stop,
}

pub enum EngineEvent {
    Data(Vec<u8>),
    Error(String),
    Closed,
}

fn to_serialport_parity(p: MonitorParity) -> serialport::Parity {
    match p {
        MonitorParity::None => serialport::Parity::None,
        MonitorParity::Even => serialport::Parity::Even,
        MonitorParity::Odd => serialport::Parity::Odd,
    }
}

fn map_open_err(port: &str, e: serialport::Error) -> AppError {
    match e.kind() {
        serialport::ErrorKind::NoDevice => AppError::PortDisappeared { port: port.to_string() },
        _ => AppError::Io { message: e.to_string() },
    }
}

/// Opens `port_path` with `settings` applied. Synchronous — callers on the async side run
/// this via `tokio::task::spawn_blocking` so a slow or hanging open doesn't stall the
/// runtime.
pub fn open_port(port_path: &str, settings: &SerialMonitorSettings) -> Result<Box<dyn serialport::SerialPort>, AppError> {
    let mut port = serialport::new(port_path, settings.baud)
        .parity(to_serialport_parity(settings.parity))
        .timeout(Duration::from_millis(50))
        .open()
        .map_err(|e| map_open_err(port_path, e))?;
    if let Some(rts) = settings.rts {
        let _ = port.write_request_to_send(rts);
    }
    if let Some(dtr) = settings.dtr {
        let _ = port.write_data_terminal_ready(dtr);
    }
    Ok(port)
}

/// Spawns the blocking reader/writer thread for an already-open port and returns a sender
/// for outgoing commands (`Send`/`Stop`). The thread polls for a pending command, then
/// attempts a short-timeout read, in a loop, so it stays responsive to `Stop` without
/// busy-spinning. Always emits exactly one `EngineEvent::Closed` as its last message.
pub fn spawn_io_thread(
    mut port: Box<dyn serialport::SerialPort>,
    events: tokio::sync::mpsc::UnboundedSender<EngineEvent>,
) -> std::sync::mpsc::Sender<MonitorCommand> {
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<MonitorCommand>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        'outer: loop {
            loop {
                match cmd_rx.try_recv() {
                    Ok(MonitorCommand::Stop) => break 'outer,
                    Ok(MonitorCommand::Send(bytes)) => {
                        if port.write_all(&bytes).is_err() {
                            let _ = events.send(EngineEvent::Error("write to serial port failed".into()));
                            break 'outer;
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => break 'outer,
                }
            }
            match port.read(&mut buf) {
                Ok(0) => {}
                Ok(n) => {
                    if events.send(EngineEvent::Data(buf[..n].to_vec())).is_err() {
                        break 'outer; // nobody listening anymore
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => {
                    let _ = events.send(EngineEvent::Error(e.to_string()));
                    break 'outer;
                }
            }
        }
        let _ = events.send(EngineEvent::Closed);
    });
    cmd_tx
}

#[cfg(test)]
mod tests {
    use super::*;

    const INI: &str = "\
[env]
monitor_speed = 115200
monitor_eol = LF

[env:esp32dev]
platform = espressif32
board = esp32dev
monitor_rts = 0
monitor_dtr = 0

[env:legacy]
platform = atmelavr
board = uno
monitor_parity = E
monitor_echo = yes
";

    #[test]
    fn reads_settings_with_env_fallback() {
        let s = settings_from_ini(INI, "esp32dev");
        assert_eq!(s.baud, 115200);
        assert_eq!(s.eol, "LF");
        assert_eq!(s.rts, Some(false));
        assert_eq!(s.dtr, Some(false));
    }

    #[test]
    fn unset_env_falls_back_to_all_defaults() {
        let s = settings_from_ini("", "does-not-exist");
        assert_eq!(s, SerialMonitorSettings::default());
    }

    #[test]
    fn parses_parity_and_echo() {
        let s = settings_from_ini(INI, "legacy");
        assert_eq!(s.parity, MonitorParity::Even);
        assert!(s.echo);
        // inherited from [env], not overridden in [env:legacy]
        assert_eq!(s.baud, 115200);
    }

    #[test]
    fn unsupported_parity_values_fall_back_to_none() {
        let ini = "[env:x]\nmonitor_parity = S\n";
        assert_eq!(settings_from_ini(ini, "x").parity, MonitorParity::None);
    }

    #[test]
    fn eol_bytes_maps_known_values_and_defaults_to_crlf() {
        assert_eq!(eol_bytes("LF"), b"\n");
        assert_eq!(eol_bytes("cr"), b"\r");
        assert_eq!(eol_bytes("CRLF"), b"\r\n");
        assert_eq!(eol_bytes("weird"), b"\r\n");
    }

    #[test]
    fn ring_buffer_retains_pushed_bytes_under_the_cap() {
        let mut rb = RingBuffer::new(100, 1024);
        rb.push(b"hello\n");
        rb.push(b"world\n");
        assert_eq!(rb.as_bytes(), b"hello\nworld\n");
    }

    #[test]
    fn ring_buffer_evicts_oldest_bytes_once_over_the_byte_cap() {
        let mut rb = RingBuffer::new(100, 10);
        rb.push(b"0123456789"); // exactly at cap
        rb.push(b"X"); // pushes it over — oldest byte(s) evicted
        assert_eq!(rb.len_bytes(), 10);
        assert_eq!(rb.as_bytes(), b"123456789X");
    }

    #[test]
    fn ring_buffer_evicts_oldest_lines_once_over_the_line_cap() {
        let mut rb = RingBuffer::new(2, 1024);
        rb.push(b"one\n");
        rb.push(b"two\n");
        rb.push(b"three\n");
        assert_eq!(rb.as_bytes(), b"two\nthree\n");
    }

    #[test]
    fn clear_empties_the_buffer() {
        let mut rb = RingBuffer::new(100, 1024);
        rb.push(b"hello\n");
        rb.clear();
        assert_eq!(rb.len_bytes(), 0);
        assert!(rb.as_bytes().is_empty());
    }

    #[test]
    fn parses_bool_flags_leniently() {
        assert_eq!(parse_bool_flag("1"), Some(true));
        assert_eq!(parse_bool_flag("yes"), Some(true));
        assert_eq!(parse_bool_flag("0"), Some(false));
        assert_eq!(parse_bool_flag("no"), Some(false));
        assert_eq!(parse_bool_flag("maybe"), None);
    }
}
