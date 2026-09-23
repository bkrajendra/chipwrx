//! Property test (`CLAUDE.md` Testing: "The INI writer gets a property test: parse→write
//! is byte-identical across the fixture corpus"). Every real `.ini` file under
//! `tests/fixtures/` must survive `parse(raw).raw() == raw` — and, since the writer only
//! ever runs through `patch::apply_edit`, a touch-then-revert round trip through it must
//! also come back byte-identical, over the same corpus.

use vibe_hardware_lib::core::ini::parse::parse;
use vibe_hardware_lib::core::ini::patch::{apply_edit, IniEdit};

fn fixture_corpus() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "platformio-real-sample.ini",
            include_str!("../../tests/fixtures/platformio-real-sample.ini"),
        ),
        (
            "platformio-real-multienv.ini",
            include_str!("../../tests/fixtures/platformio-real-multienv.ini"),
        ),
    ]
}

#[test]
fn every_fixture_round_trips_byte_identical_with_zero_edits() {
    for (name, raw) in fixture_corpus() {
        let doc = parse(raw);
        assert_eq!(doc.raw(), raw, "{name} did not round-trip byte-identical");
    }
}

#[test]
fn every_fixture_survives_a_set_then_remove_round_trip_on_every_declared_section() {
    for (name, raw) in fixture_corpus() {
        let doc = parse(raw);
        for section in &doc.sections {
            if section.name.is_empty() {
                continue; // the preamble pseudo-section isn't user-addressable
            }
            let probe_key = "__vibe_hw_roundtrip_probe__";
            let added = apply_edit(
                raw,
                &IniEdit::Set {
                    section: section.name.clone(),
                    name: probe_key.into(),
                    values: vec!["1".into()],
                },
            );
            assert_ne!(added, raw, "{name}/[{}] Set didn't change anything", section.name);

            let reverted = apply_edit(
                &added,
                &IniEdit::Remove {
                    section: section.name.clone(),
                    name: probe_key.into(),
                },
            );
            // A file with no trailing newline at EOF is the one documented exception, and
            // only when the *edited* section is the one at EOF: appending anything after
            // the final line must first give it one (otherwise the new content would run
            // onto the old line with no separator), and removing what was appended doesn't
            // retroactively un-normalize that — the file ends up with exactly one added
            // trailing newline, not perfectly byte-identical. Editing any earlier section
            // is unaffected and must still be exactly byte-identical.
            let section_is_at_eof = section.end == doc.lines.len();
            let expected = if raw.ends_with('\n') || !section_is_at_eof {
                raw.to_string()
            } else {
                format!("{raw}\n")
            };
            assert_eq!(reverted, expected, "{name}/[{}] add+remove did not round-trip as expected", section.name);
        }
    }
}
