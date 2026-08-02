//! The password store: directory layout, name → path mapping, gpg-id
//! resolution, and tree listing. Mirrors pass(1): entries are `NAME.gpg`
//! files under the store root, recipients come from the nearest `.gpg-id`
//! file walking up from the entry toward the root.

use anyhow::{Context, Result, anyhow, bail};
use std::fs;
use std::path::{Path, PathBuf};

use crate::color;

/// Directory under the store root that holds Tera entry templates.
pub const TEMPLATES_DIR: &str = ".templates";

pub struct Store {
    root: PathBuf,
}

impl Store {
    /// Resolve the store location: `--store` flag, then `$PASSWORD_STORE_DIR`,
    /// then `~/.password-store`. Does not require the directory to exist —
    /// `init` creates it; everything else calls `require_initialized`.
    pub fn locate(cli_override: Option<&str>) -> Result<Self> {
        let root = if let Some(dir) = cli_override {
            PathBuf::from(dir)
        } else if let Some(dir) = std::env::var_os("PASSWORD_STORE_DIR") {
            PathBuf::from(dir)
        } else {
            let home = std::env::var_os("HOME").context("HOME is not set and no store directory was given")?;
            Path::new(&home).join(".password-store")
        };
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Error out unless the store has been initialized (root exists and a
    /// `.gpg-id` is reachable from it).
    pub fn require_initialized(&self) -> Result<()> {
        if !self.root.join(".gpg-id").is_file() {
            bail!(
                "password store is empty (no .gpg-id in {}) — try \"rspass init <gpg-id>\"",
                self.root.display()
            );
        }
        Ok(())
    }

    /// Reject pass names that escape the store ("..", absolute paths, or a
    /// sneaky ".gpg" injection). Same check pass(1) does.
    pub fn check_sneaky(name: &str) -> Result<()> {
        let p = Path::new(name);
        if p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            bail!("sneaky path rejected: {name}");
        }
        Ok(())
    }

    /// Filesystem path of an entry's .gpg file.
    pub fn entry_path(&self, name: &str) -> Result<PathBuf> {
        Self::check_sneaky(name)?;
        Ok(self.root.join(format!("{name}.gpg")))
    }

    /// Filesystem path of a directory inside the store.
    pub fn dir_path(&self, name: &str) -> Result<PathBuf> {
        Self::check_sneaky(name)?;
        Ok(self.root.join(name))
    }

    /// GPG recipients for an entry: nearest `.gpg-id` file walking up from
    /// the entry's directory to the store root, one key id per line.
    pub fn gpg_ids_for(&self, name: &str) -> Result<Vec<String>> {
        Self::check_sneaky(name)?;
        let mut dir = self.root.join(name);
        // Start from the entry's parent directory.
        dir.pop();
        loop {
            let candidate = dir.join(".gpg-id");
            if candidate.is_file() {
                return read_gpg_id_file(&candidate);
            }
            if dir == self.root || !dir.starts_with(&self.root) {
                break;
            }
            dir.pop();
        }
        // Walk ended before the root candidate was checked when name has no
        // subdirectory; check the root explicitly.
        let root_ids = self.root.join(".gpg-id");
        if root_ids.is_file() {
            return read_gpg_id_file(&root_ids);
        }
        bail!(
            "no .gpg-id found for {name} — store not initialized? try \"rspass init <gpg-id>\""
        )
    }

    /// All entry names (relative, without .gpg suffix) under a subfolder,
    /// sorted. Hidden files and directories (like .git, .templates) are
    /// skipped.
    pub fn list_entries(&self, subfolder: Option<&str>) -> Result<Vec<String>> {
        let base = match subfolder {
            Some(s) => self.dir_path(s)?,
            None => self.root.clone(),
        };
        let mut names = Vec::new();
        collect_entries(&base, &self.root, &mut names)?;
        names.sort();
        Ok(names)
    }

    /// Render the store as a tree, like `pass ls`. Returns the printed lines.
    pub fn tree(&self, subfolder: Option<&str>) -> Result<Vec<String>> {
        let base = match subfolder {
            Some(s) => self.dir_path(s)?,
            None => self.root.clone(),
        };
        if !base.is_dir() {
            bail!("{} is not in the password store", subfolder.unwrap_or(""));
        }
        let header = match subfolder {
            Some(s) => s.to_owned(),
            None => "Password Store".to_owned(),
        };
        let mut lines = vec![color::bold_blue(&header).into_owned()];
        render_tree(&base, "", &mut lines)?;
        Ok(lines)
    }
}

fn read_gpg_id_file(path: &Path) -> Result<Vec<String>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let ids: Vec<String> = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(ToOwned::to_owned)
        .collect();
    if ids.is_empty() {
        bail!("{} contains no key ids", path.display());
    }
    Ok(ids)
}

/// Directory entries sorted by name, hidden files skipped.
fn sorted_visible_entries(dir: &Path) -> Result<Vec<fs::DirEntry>> {
    let mut entries: Vec<fs::DirEntry> = fs::read_dir(dir)
        .with_context(|| format!("failed to read directory {}", dir.display()))?
        .collect::<std::io::Result<Vec<fs::DirEntry>>>()
        .with_context(|| format!("failed to read directory {}", dir.display()))?
        .into_iter()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .collect();
    entries.sort_by_key(fs::DirEntry::file_name);
    Ok(entries)
}

fn collect_entries(dir: &Path, root: &Path, out: &mut Vec<String>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in sorted_visible_entries(dir)? {
        let path = entry.path();
        if path.is_dir() {
            collect_entries(&path, root, out)?;
        } else if path.extension().is_some_and(|e| e == "gpg") {
            let rel = path
                .strip_prefix(root)
                .map_err(|_| anyhow!("entry {} outside store root", path.display()))?;
            let name = rel.with_extension("");
            out.push(name.to_string_lossy().into_owned());
        }
    }
    Ok(())
}

fn render_tree(dir: &Path, prefix: &str, lines: &mut Vec<String>) -> Result<()> {
    let entries = sorted_visible_entries(dir)?;
    let last_idx = entries.len().saturating_sub(1);
    for (i, entry) in entries.iter().enumerate() {
        let path = entry.path();
        let is_last = i == last_idx;
        let connector = if is_last { "└── " } else { "├── " };
        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().into_owned();
            lines.push(format!("{prefix}{connector}{}", color::bold_blue(&name)));
            let child_prefix = format!("{prefix}{}", if is_last { "    " } else { "│   " });
            render_tree(&path, &child_prefix, lines)?;
        } else if path.extension().is_some_and(|e| e == "gpg") {
            let name = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            lines.push(format!("{prefix}{connector}{name}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sneaky_paths_are_rejected() {
        assert!(Store::check_sneaky("../escape").is_err());
        assert!(Store::check_sneaky("a/../../escape").is_err());
        assert!(Store::check_sneaky("/absolute").is_err());
        assert!(Store::check_sneaky("ok/nested/name").is_ok());
        assert!(Store::check_sneaky("plain").is_ok());
    }

    #[test]
    fn gpg_id_resolution_walks_up() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join(".gpg-id"), "root-key\n").unwrap();
        fs::create_dir_all(root.join("work/sub")).unwrap();
        fs::write(root.join("work/.gpg-id"), "work-key\nsecond-key\n").unwrap();

        let store = Store { root: root.to_path_buf() };
        assert_eq!(store.gpg_ids_for("top").unwrap(), vec!["root-key"]);
        assert_eq!(
            store.gpg_ids_for("work/sub/entry").unwrap(),
            vec!["work-key", "second-key"]
        );
    }

    #[test]
    fn list_entries_skips_hidden_and_sorts() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("b")).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("b/two.gpg"), "").unwrap();
        fs::write(root.join("a.gpg"), "").unwrap();
        fs::write(root.join(".hidden.gpg"), "").unwrap();
        fs::write(root.join(".git/x.gpg"), "").unwrap();
        fs::write(root.join("notes.txt"), "").unwrap();

        let store = Store { root: root.to_path_buf() };
        assert_eq!(store.list_entries(None).unwrap(), vec!["a", "b/two"]);
    }
}
