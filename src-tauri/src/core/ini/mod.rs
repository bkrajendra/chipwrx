//! `platformio.ini` reading, editing, and the effective-config/lint/schema support around
//! it (`ARCHITECTURE.md` §3 `core/ini`, `SPEC.md` §5.6 FR-INI-1..11).
//!
//! Three format-preserving operations plus the read-only effective view:
//! - [`parse`] — `read_declared`: format-preserving parse, every source line kept verbatim.
//! - [`patch`] — surgical edits over the parsed structure.
//! - [`effective`] — reshapes `pio project config --json-output`'s resolved output.
//! - [`document`] — combines the two into the IPC-facing `IniDocument` (`inheritedFrom`
//!   badges included).
//! - [`lint`] — parses `pio project config --lint`'s human-readable output.
//! - [`schema`] — the option schema (Python one-liner extraction, version-keyed cache,
//!   bundled fallback).

pub mod document;
pub mod effective;
pub mod lint;
pub mod parse;
pub mod patch;
pub mod schema;
