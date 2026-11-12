// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Directory trees: how much space one takes, and copying one aside before
//! an application is removed.
//!
//! Symbolic links are never followed. A Windows prefix links its users'
//! folders to places that may be outside it, and a copy that followed them
//! could copy the user's whole home; a link is recorded as skipped instead.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::codes::{Code, Failure};

/// The space a tree takes: the sum of its regular files' sizes. Links are
/// not followed; a tree that cannot be read counts what could be.
pub fn size(root: &Path) -> u64 {
    let Ok(metadata) = fs::symlink_metadata(root) else {
        return 0;
    };
    if metadata.is_file() {
        return metadata.len();
    }
    if !metadata.is_dir() {
        return 0;
    }
    let Ok(listing) = fs::read_dir(root) else {
        return 0;
    };
    listing
        .filter_map(Result::ok)
        .map(|entry| size(&entry.path()))
        .sum()
}

/// What a copy did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Copied {
    /// Regular files copied.
    pub files: u64,
    /// Bytes copied.
    pub bytes: u64,
    /// Symbolic links and special files that were not copied, by their
    /// path relative to the source.
    pub skipped: Vec<String>,
}

/// Copies the tree `source` to `target`, which must not exist yet. Regular
/// files and directories are copied; everything else is listed as skipped.
pub fn copy(source: &Path, target: &Path) -> Result<Copied, Failure> {
    if fs::symlink_metadata(target).is_ok() {
        return Err(Failure::new(
            Code::StepFailed,
            format!(
                "{} already exists; nothing was copied over it",
                target.display()
            ),
        ));
    }
    let mut copied = Copied::default();
    copy_into(source, target, Path::new(""), &mut copied)?;
    Ok(copied)
}

fn copy_into(
    source: &Path,
    target: &Path,
    relative: &Path,
    copied: &mut Copied,
) -> Result<(), Failure> {
    let failed = |what: &str, path: &Path, error: std::io::Error| {
        Failure::new(
            Code::StepFailed,
            format!("cannot {what} {}: {error}", path.display()),
        )
    };
    let metadata = fs::symlink_metadata(source).map_err(|error| failed("read", source, error))?;
    if metadata.is_dir() {
        fs::create_dir_all(target).map_err(|error| failed("create", target, error))?;
        let mut entries: Vec<PathBuf> = fs::read_dir(source)
            .map_err(|error| failed("read", source, error))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        for entry in entries {
            let Some(name) = entry.file_name() else {
                continue;
            };
            copy_into(&entry, &target.join(name), &relative.join(name), copied)?;
        }
    } else if metadata.is_file() {
        let bytes = fs::copy(source, target).map_err(|error| failed("copy", source, error))?;
        copied.files += 1;
        copied.bytes += bytes;
    } else {
        copied.skipped.push(relative.display().to_string());
    }
    Ok(())
}

/// Every path under `root` (itself included) that exists, for a report of
/// what was left behind. At most `limit` are returned.
pub fn list(root: &Path, limit: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk(root, limit, &mut found);
    found
}

fn walk(path: &Path, limit: usize, found: &mut Vec<PathBuf>) {
    if found.len() >= limit {
        return;
    }
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    found.push(path.to_path_buf());
    if metadata.is_dir()
        && let Ok(listing) = fs::read_dir(path)
    {
        let mut entries: Vec<PathBuf> = listing
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        for entry in entries {
            walk(&entry, limit, found);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tree_is_copied_without_following_links() {
        let root = std::env::temp_dir().join(format!("tur-apps-tree-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let source = root.join("source");
        fs::create_dir_all(source.join("Documents/sub")).unwrap();
        fs::write(source.join("Documents/letter.txt"), b"Dear reader").unwrap();
        fs::write(source.join("Documents/sub/b"), b"12").unwrap();
        fs::write(root.join("outside"), b"not to be copied").unwrap();
        std::os::unix::fs::symlink(root.join("outside"), source.join("link")).unwrap();

        assert_eq!(size(&source), 13);
        let target = root.join("kept");
        let copied = copy(&source, &target).unwrap();
        assert_eq!(copied.files, 2);
        assert_eq!(copied.bytes, 13);
        assert_eq!(copied.skipped, ["link"]);
        assert_eq!(
            fs::read(target.join("Documents/letter.txt")).unwrap(),
            b"Dear reader"
        );
        assert!(fs::symlink_metadata(target.join("link")).is_err());
        // Never over something that is there.
        assert_eq!(copy(&source, &target).unwrap_err().code, Code::StepFailed);
        assert_eq!(list(&target, 100).len(), 5);
        assert_eq!(list(&root.join("missing"), 100).len(), 0);
        assert_eq!(size(&root.join("missing")), 0);
    }
}
