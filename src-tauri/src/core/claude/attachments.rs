//! Attachments (`FR-CHAT-9`): "The user may attach a datasheet PDF, a photo of wiring, or
//! a captured serial log to a prompt. Attachments are copied into `.vibe/attachments/` and
//! referenced by relative path in the prompt text so Claude's own `Read` tool fetches
//! them."

use crate::error::{AppError, Result};
use std::path::{Path, PathBuf};

const ATTACHMENTS_DIR: &str = ".vibe/attachments";

/// Copies `source` into `<workspace>/.vibe/attachments/<name>`, disambiguating a name
/// collision by appending `-2`, `-3`, ... before the extension rather than overwriting an
/// earlier attachment with the same file name. Returns the workspace-relative path (always
/// forward-slash-joined, regardless of platform) to store in `TurnRequest.attachments`.
pub fn add(workspace: &Path, source: &Path) -> Result<String> {
    let file_name = source.file_name().ok_or_else(|| AppError::Io {
        message: format!("{} has no file name", source.display()),
    })?;
    let dir = workspace.join(ATTACHMENTS_DIR);
    std::fs::create_dir_all(&dir)?;

    let stem = Path::new(file_name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = Path::new(file_name).extension().map(|s| s.to_string_lossy().into_owned());

    let mut candidate = PathBuf::from(file_name);
    let mut n = 1u32;
    while dir.join(&candidate).exists() {
        n += 1;
        candidate = PathBuf::from(match &ext {
            Some(e) => format!("{stem}-{n}.{e}"),
            None => format!("{stem}-{n}"),
        });
    }

    std::fs::copy(source, dir.join(&candidate))?;
    Ok(format!("{ATTACHMENTS_DIR}/{}", candidate.display()).replace('\\', "/"))
}

/// Appends attachment references to the user's own prompt text — never replaces it, and a
/// prompt with no attachments passes through unchanged (byte-identical), so this is safe to
/// call unconditionally at the argv-build site.
pub fn augment_prompt(prompt: &str, attachments: &[String]) -> String {
    if attachments.is_empty() {
        return prompt.to_string();
    }
    let list = attachments.iter().map(|a| format!("- {a}")).collect::<Vec<_>>().join("\n");
    format!("{prompt}\n\nAttachments:\n{list}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vibe-hw-attachments-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn copies_a_file_and_returns_its_relative_path() {
        let workspace = tempdir();
        let source_dir = tempdir();
        let source = source_dir.join("datasheet.pdf");
        std::fs::write(&source, b"pdf bytes").unwrap();

        let relative = add(&workspace, &source).expect("add");
        assert_eq!(relative, ".vibe/attachments/datasheet.pdf");
        assert_eq!(std::fs::read(workspace.join(".vibe/attachments/datasheet.pdf")).unwrap(), b"pdf bytes");
    }

    #[test]
    fn a_name_collision_gets_a_numbered_suffix_not_overwritten() {
        let workspace = tempdir();
        let source_dir = tempdir();
        let source = source_dir.join("photo.jpg");
        std::fs::write(&source, b"first").unwrap();
        let first = add(&workspace, &source).expect("add first");

        std::fs::write(&source, b"second").unwrap();
        let second = add(&workspace, &source).expect("add second");

        assert_eq!(first, ".vibe/attachments/photo.jpg");
        assert_eq!(second, ".vibe/attachments/photo-2.jpg");
        assert_eq!(std::fs::read(workspace.join(&first)).unwrap(), b"first");
        assert_eq!(std::fs::read(workspace.join(".vibe/attachments/photo-2.jpg")).unwrap(), b"second");
    }

    #[test]
    fn a_file_with_no_extension_still_gets_a_numbered_suffix() {
        let workspace = tempdir();
        let source_dir = tempdir();
        let source = source_dir.join("serial-log");
        std::fs::write(&source, b"one").unwrap();
        add(&workspace, &source).expect("add first");
        std::fs::write(&source, b"two").unwrap();
        let second = add(&workspace, &source).expect("add second");
        assert_eq!(second, ".vibe/attachments/serial-log-2");
    }

    #[test]
    fn augment_prompt_is_unchanged_with_no_attachments() {
        assert_eq!(augment_prompt("hello", &[]), "hello");
    }

    #[test]
    fn augment_prompt_lists_every_attachment_by_relative_path() {
        let augmented = augment_prompt("look at this", &[".vibe/attachments/a.pdf".into(), ".vibe/attachments/b.jpg".into()]);
        assert_eq!(augmented, "look at this\n\nAttachments:\n- .vibe/attachments/a.pdf\n- .vibe/attachments/b.jpg");
    }
}
