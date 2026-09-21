//! Generated `CLAUDE.md` (`FR-PROJ-8`, `DATA-MODEL.md` §11). Written at the workspace root
//! on creation and on board change. Content between the markers is regenerated; anything
//! outside them — the user's own edits — survives regeneration untouched.

use crate::core::pio::boards::BoardBrief;

const BEGIN_MARKER: &str = "<!-- vibe-hardware:begin (generated — edits inside this block are overwritten) -->";
const END_MARKER: &str = "<!-- vibe-hardware:end -->";

fn format_mhz(fcpu_hz: u64) -> String {
    format!("{} MHz", fcpu_hz / 1_000_000)
}

fn format_flash(rom_bytes: u64) -> String {
    if rom_bytes >= 1024 * 1024 {
        format!("{} MB", rom_bytes / (1024 * 1024))
    } else {
        format!("{} KB", rom_bytes / 1024)
    }
}

fn format_ram(ram_bytes: u64) -> String {
    format!("{} KB", ram_bytes / 1024)
}

/// Just the generated block (without the surrounding markers) — used by both a
/// from-scratch write and a regeneration splice.
fn generated_body(board: &BoardBrief, framework: &str, platformio_ini: &str) -> String {
    let mut s = String::new();
    s.push_str("# Hardware context\n\n");
    s.push_str(&format!(
        "Target board: **{}** — {} ({})\n",
        board.id, board.name, board.vendor
    ));
    s.push_str(&format!(
        "MCU: {} @ {} · Flash {} · RAM {}\n",
        board.mcu,
        format_mhz(board.fcpu),
        format_flash(board.rom),
        format_ram(board.ram)
    ));
    s.push_str(&format!(
        "Frameworks available: {} · Active: {}\n",
        board.frameworks.join(", "),
        framework
    ));
    if !board.connectivity.is_empty() {
        s.push_str(&format!("Connectivity: {}\n", board.connectivity.join(", ")));
    }
    s.push('\n');
    s.push_str("## Build system\n");
    s.push_str(
        "PlatformIO. Do not invent a Makefile or CMakeLists — `platformio.ini` is the build\nconfiguration.\n\n",
    );
    s.push_str("Current `platformio.ini`:\n\n");
    s.push_str("```ini\n");
    s.push_str(platformio_ini.trim_end());
    s.push_str("\n```\n\n");
    s.push_str("## House rules\n");
    s.push_str(
        "- Application code goes in `src/`. Shared headers in `include/`. Private libraries in\n  `lib/<Name>/`. Tests in `test/`.\n",
    );
    s.push_str(
        "- Add every external library to `lib_deps` in `platformio.ini`. Never vendor sources\n  into `src/`.\n",
    );
    s.push_str(
        "- This target is memory-constrained: avoid dynamic allocation inside `loop()`, prefer\n  fixed-size buffers, and keep ISRs short and `IRAM_ATTR` where the platform requires it.\n",
    );
    s.push_str("- Do not write to `.pio/` or `.vibe/`.\n");
    s.push_str(
        "- Do not change `board`, `platform`, `upload_protocol`, or flash layout options unless\n  explicitly asked — those can make the device unflashable.\n",
    );
    s.push_str(
        "- After changing code, do not run the build yourself; the user triggers Build and Upload\n  from the app.\n",
    );
    s
}

fn wrapped_block(board: &BoardBrief, framework: &str, platformio_ini: &str) -> String {
    format!(
        "{BEGIN_MARKER}\n{}{END_MARKER}\n",
        generated_body(board, framework, platformio_ini)
    )
}

/// `existing` is the current file content, if any. `None` (first creation) just writes the
/// block; `Some` splices the block into the existing markers (or appends them, if this file
/// predates the app) so anything the user wrote outside them is untouched.
pub fn regenerate(existing: Option<&str>, board: &BoardBrief, framework: &str, platformio_ini: &str) -> String {
    let block = wrapped_block(board, framework, platformio_ini);
    match existing {
        None => block,
        Some(text) => splice(text, &block),
    }
}

fn splice(existing: &str, new_block: &str) -> String {
    match (existing.find(BEGIN_MARKER), existing.find(END_MARKER)) {
        (Some(begin), Some(end)) if end >= begin => {
            let end_of_marker = end + END_MARKER.len();
            let before = &existing[..begin];
            let after = &existing[end_of_marker..];
            let after = after.strip_prefix('\n').unwrap_or(after);
            format!("{before}{new_block}{after}")
        }
        _ => {
            // No prior block (a hand-written file, or one predating this feature) — append,
            // separated by a blank line if the file doesn't already end with one.
            let sep = if existing.ends_with("\n\n") || existing.is_empty() {
                ""
            } else if existing.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            };
            format!("{existing}{sep}{new_block}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn esp32dev() -> BoardBrief {
        BoardBrief {
            id: "esp32dev".into(),
            name: "Espressif ESP32 Dev Module".into(),
            platform: "espressif32".into(),
            mcu: "ESP32".into(),
            fcpu: 240_000_000,
            ram: 327_680,
            rom: 4_194_304,
            frameworks: vec!["arduino".into(), "espidf".into()],
            vendor: "Espressif".into(),
            url: "https://example.com".into(),
            connectivity: vec!["wifi".into(), "bluetooth".into(), "ethernet".into(), "can".into()],
            debug_tools: vec![],
        }
    }

    const INI: &str = "[env:esp32dev]\nplatform = espressif32\nboard = esp32dev\nframework = arduino\nmonitor_speed = 115200\n";

    #[test]
    fn matches_the_documented_template_exactly() {
        let out = regenerate(None, &esp32dev(), "arduino", INI);
        let expected = "<!-- vibe-hardware:begin (generated — edits inside this block are overwritten) -->\n\
# Hardware context\n\n\
Target board: **esp32dev** — Espressif ESP32 Dev Module (Espressif)\n\
MCU: ESP32 @ 240 MHz · Flash 4 MB · RAM 320 KB\n\
Frameworks available: arduino, espidf · Active: arduino\n\
Connectivity: wifi, bluetooth, ethernet, can\n\n\
## Build system\n\
PlatformIO. Do not invent a Makefile or CMakeLists — `platformio.ini` is the build\n\
configuration.\n\n\
Current `platformio.ini`:\n\n\
```ini\n\
[env:esp32dev]\n\
platform = espressif32\n\
board = esp32dev\n\
framework = arduino\n\
monitor_speed = 115200\n\
```\n\n\
## House rules\n\
- Application code goes in `src/`. Shared headers in `include/`. Private libraries in\n  `lib/<Name>/`. Tests in `test/`.\n\
- Add every external library to `lib_deps` in `platformio.ini`. Never vendor sources\n  into `src/`.\n\
- This target is memory-constrained: avoid dynamic allocation inside `loop()`, prefer\n  fixed-size buffers, and keep ISRs short and `IRAM_ATTR` where the platform requires it.\n\
- Do not write to `.pio/` or `.vibe/`.\n\
- Do not change `board`, `platform`, `upload_protocol`, or flash layout options unless\n  explicitly asked — those can make the device unflashable.\n\
- After changing code, do not run the build yourself; the user triggers Build and Upload\n  from the app.\n\
<!-- vibe-hardware:end -->\n";
        assert_eq!(out, expected);
    }

    #[test]
    fn regeneration_preserves_user_content_outside_the_markers() {
        let board = esp32dev();
        let first = regenerate(None, &board, "arduino", INI);
        let hand_edited = format!("{first}\n## My own notes\n\nDon't forget to solder the antenna.\n");

        // Board changes (e.g. RAM bumped to simulate a swap); regenerating must still leave
        // the user's own trailing section intact.
        let mut board2 = board.clone();
        board2.ram = 520_000;
        let regenerated = regenerate(Some(&hand_edited), &board2, "arduino", INI);

        assert!(regenerated.contains("Don't forget to solder the antenna."));
        assert!(regenerated.contains("RAM 507 KB")); // 520000 / 1024 = 507
        assert!(!regenerated.contains("RAM 320 KB"), "stale generated content should be replaced");
    }

    #[test]
    fn appends_the_block_when_the_file_predates_the_markers() {
        let hand_written = "# My Project\n\nSome notes I wrote before using this app.\n";
        let out = regenerate(Some(hand_written), &esp32dev(), "arduino", INI);
        assert!(out.starts_with(hand_written));
        assert!(out.contains(BEGIN_MARKER));
        assert!(out.contains(END_MARKER));
    }

    #[test]
    fn small_flash_boards_render_in_kb_not_mb() {
        let mut board = esp32dev();
        board.rom = 32_256; // Arduino Uno-sized
        let out = regenerate(None, &board, "arduino", INI);
        assert!(out.contains("Flash 31 KB"));
    }
}
