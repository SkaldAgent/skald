mod edit_file;
mod grep_files;
mod insert_at_line;
mod list_files;
mod read_file;
mod replace_lines;
mod search_file;
mod write_file;

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::core::tools::ToolRegistry;

pub use edit_file::EditFile;
pub use grep_files::GrepFiles;
pub use insert_at_line::InsertAtLine;
pub use list_files::ListFiles;
pub use read_file::ReadFile;
pub use replace_lines::ReplaceLines;
pub use write_file::WriteFile;

/// Resolve a user-supplied path:
/// - starts with `/`  → absolute path, used as-is
/// - otherwise        → relative to the process working directory (project root)
pub fn resolve(user_path: &str) -> Result<PathBuf> {
    let p = PathBuf::from(user_path);
    if p.is_absolute() {
        Ok(p)
    } else {
        let cwd = std::env::current_dir()
            .context("Failed to read current working directory")?;
        Ok(cwd.join(p))
    }
}

pub(super) fn read_to_string(user_path: &str) -> Result<String> {
    let abs = resolve(user_path)?;
    std::fs::read_to_string(&abs)
        .with_context(|| format!("Cannot read file: {user_path}"))
}

pub(super) fn write_string(user_path: &str, content: &str) -> Result<()> {
    let abs = resolve(user_path)?;
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }
    std::fs::write(&abs, content)
        .with_context(|| format!("Failed to write: {}", abs.display()))
}

pub fn register_all(registry: &mut ToolRegistry) {
    registry.register(EditFile::new());
    registry.register(GrepFiles::new());
    registry.register(InsertAtLine::new());
    registry.register(ListFiles::new());
    registry.register(ReadFile::new());
    registry.register(ReplaceLines::new());
    registry.register(WriteFile::new());
}
