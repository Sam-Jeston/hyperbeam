//! Copying a directory out of a container image into a volume.
//!
//! A sandbox's writable volumes are mounted over paths the image may already
//! have filled, and an environment image has no agent of its own. An init
//! container that runs this fills the gap: it sees the image's files at their
//! own path, the volume beside them, and copies across.

use anyhow::{Context, Result};
use hyperbeam_core::protocol::HOME_SKIPPED_MARKER;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The image has nothing there.
    Nothing,
    Copied { files: u64, skipped: u64 },
    /// More than the volume is allowed to take; left out, with a marker in
    /// the destination saying so.
    TooLarge { bytes: u64 },
}

/// Copy the contents of `from` into `to`. Never fails for what the image
/// holds: files that cannot be read are skipped and counted, because a
/// sandbox with part of a home directory is better than none.
pub fn seed(from: &Path, to: &Path, max_bytes: Option<u64>) -> Result<Outcome> {
    if !from.is_dir() || fs::read_dir(from).map(|mut entries| entries.next().is_none()).unwrap_or(true) {
        return Ok(Outcome::Nothing);
    }
    fs::create_dir_all(to).with_context(|| format!("could not create {}", to.display()))?;
    if let Some(max) = max_bytes {
        let bytes = size(from);
        if bytes > max {
            fs::write(to.join(HOME_SKIPPED_MARKER), bytes.to_string())?;
            return Ok(Outcome::TooLarge { bytes });
        }
    }
    let (mut files, mut skipped) = (0, 0);
    copy_tree(from, to, &mut files, &mut skipped);
    Ok(Outcome::Copied { files, skipped })
}

fn size(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else { return 0 };
    entries
        .flatten()
        .map(|entry| match entry.metadata() {
            Ok(meta) if meta.is_dir() => size(&entry.path()),
            Ok(meta) if meta.is_file() => meta.len(),
            _ => 0,
        })
        .sum()
}

fn copy_tree(from: &Path, to: &Path, files: &mut u64, skipped: &mut u64) {
    let Ok(entries) = fs::read_dir(from) else {
        *skipped += 1;
        return;
    };
    for entry in entries.flatten() {
        let (source, dest) = (entry.path(), to.join(entry.file_name()));
        // `DirEntry::metadata` does not follow symlinks.
        let Ok(meta) = entry.metadata() else {
            *skipped += 1;
            continue;
        };
        let copied: io::Result<()> = if meta.is_dir() {
            fs::create_dir(&dest).or_else(|err| if dest.is_dir() { Ok(()) } else { Err(err) }).and_then(|()| {
                copy_tree(&source, &dest, files, skipped);
                // The copy belongs to the sandbox user, who must be able to use it.
                fs::set_permissions(&dest, fs::Permissions::from_mode(meta.permissions().mode() & 0o777 | 0o700))
            })
        } else if meta.is_symlink() {
            fs::read_link(&source).and_then(|target| std::os::unix::fs::symlink(target, &dest))
        } else if meta.is_file() {
            fs::copy(&source, &dest).map(|_| *files += 1)
        } else {
            // Sockets, devices and pipes mean nothing in another container.
            Ok(())
        };
        if copied.is_err() {
            *skipped += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("hb-seed-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_tree_is_copied_with_its_links_and_modes() {
        let root = scratch("copy");
        let (from, to) = (root.join("from"), root.join("to"));
        fs::create_dir_all(from.join(".cargo/bin")).unwrap();
        fs::write(from.join(".cargo/bin/cargo"), "#!/bin/sh\n").unwrap();
        fs::set_permissions(from.join(".cargo/bin/cargo"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(from.join(".profile"), "export A=1\n").unwrap();
        std::os::unix::fs::symlink("cargo", from.join(".cargo/bin/rustc")).unwrap();
        fs::set_permissions(from.join(".cargo"), fs::Permissions::from_mode(0o555)).unwrap();

        assert_eq!(seed(&from, &to, Some(1 << 20)).unwrap(), Outcome::Copied { files: 2, skipped: 0 });
        assert_eq!(fs::read_to_string(to.join(".profile")).unwrap(), "export A=1\n");
        assert_eq!(fs::metadata(to.join(".cargo/bin/cargo")).unwrap().permissions().mode() & 0o777, 0o755);
        assert_eq!(fs::read_link(to.join(".cargo/bin/rustc")).unwrap(), Path::new("cargo"));
        // A read-only directory in the image is still the sandbox user's to write.
        assert_eq!(fs::metadata(to.join(".cargo")).unwrap().permissions().mode() & 0o777, 0o755);

        fs::set_permissions(from.join(".cargo"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_absent_or_empty_directory_is_nothing() {
        let root = scratch("empty");
        assert_eq!(seed(&root.join("missing"), &root.join("to"), None).unwrap(), Outcome::Nothing);
        fs::create_dir(root.join("empty")).unwrap();
        assert_eq!(seed(&root.join("empty"), &root.join("to"), None).unwrap(), Outcome::Nothing);
        assert!(!root.join("to").exists());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn too_much_is_left_out_and_said_so() {
        let root = scratch("large");
        let (from, to) = (root.join("from"), root.join("to"));
        fs::create_dir_all(from.join("toolchain")).unwrap();
        fs::write(from.join("toolchain/big"), vec![0u8; 4096]).unwrap();
        assert_eq!(seed(&from, &to, Some(1024)).unwrap(), Outcome::TooLarge { bytes: 4096 });
        assert_eq!(fs::read_to_string(to.join(HOME_SKIPPED_MARKER)).unwrap(), "4096");
        assert!(!to.join("toolchain").exists());
        fs::remove_dir_all(&root).ok();
    }
}
