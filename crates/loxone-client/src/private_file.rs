//! Files that hold credentials or personal data: the token, the structure
//! cache, and for callers their config next to them. Readable by the owner
//! only (Unix), and replaced in one step, so a crash or a second process
//! never sees half a file.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// Write `data` to `path`, mode 0600 on Unix.
///
/// The data goes to a temporary file in the same directory, which then
/// replaces `path`. Where that is not possible (a read-only directory, a
/// file mounted on its own into a container) the file is rewritten in place.
/// A symlink at `path` is followed, not replaced.
pub fn write(path: &Path, data: &[u8]) -> io::Result<()> {
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    match replace(&target, data) {
        Ok(()) => Ok(()),
        Err(_) => rewrite(&target, data),
    }
}

/// Create `path` and its missing parents, mode 0700 on Unix. Directories
/// that exist keep their mode.
pub fn create_dir_all(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

fn open_private(options: &mut OpenOptions, path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn replace(target: &Path, data: &[u8]) -> io::Result<()> {
    static SEQUENCE: AtomicU32 = AtomicU32::new(0);
    let name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    let tmp: PathBuf = target.with_file_name(format!(
        ".{}.{}-{}.tmp",
        name.to_string_lossy(),
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let written = (|| {
        let mut file = open_private(OpenOptions::new().write(true).create_new(true), &tmp)?;
        file.write_all(data)?;
        file.sync_all()?;
        fs::rename(&tmp, target)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

fn rewrite(target: &Path, data: &[u8]) -> io::Result<()> {
    let mut file = open_private(
        OpenOptions::new().write(true).create(true).truncate(true),
        target,
    )?;
    // The mode only applies to a new file; tighten one that existed.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn writes_and_replaces_without_leftovers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        write(&path, b"one").unwrap();
        write(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["config.yaml"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_new_file_and_a_loose_old_one_end_up_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let fresh = dir.path().join("token.json");
        write(&fresh, b"{}").unwrap();
        assert_eq!(mode(&fresh), 0o600);

        let loose = dir.path().join("config.yaml");
        fs::write(&loose, "pass: old").unwrap();
        fs::set_permissions(&loose, fs::Permissions::from_mode(0o644)).unwrap();
        write(&loose, b"pass: new").unwrap();
        assert_eq!(mode(&loose), 0o600);
        assert_eq!(fs::read(&loose).unwrap(), b"pass: new");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_followed_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("dotfiles").join("lox.yaml");
        fs::create_dir(real.parent().unwrap()).unwrap();
        fs::write(&real, "old").unwrap();
        let link = dir.path().join("config.yaml");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        write(&link, b"new").unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(fs::read(&real).unwrap(), b"new");
        assert_eq!(mode(&real), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn a_read_only_directory_falls_back_to_rewriting_in_place() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(&path, "old").unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
        let result = write(&path, b"new");
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        result.unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(mode(&path), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn new_directories_are_owner_only_and_old_ones_keep_their_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let shared = dir.path().join("shared");
        fs::create_dir(&shared).unwrap();
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o755)).unwrap();
        let nested = shared.join("cache").join("deep");
        create_dir_all(&nested).unwrap();
        create_dir_all(&nested).unwrap();
        assert_eq!(mode(&nested), 0o700);
        assert_eq!(mode(&shared.join("cache")), 0o700);
        assert_eq!(mode(&shared), 0o755);
    }

    #[test]
    fn a_missing_directory_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(write(&dir.path().join("missing").join("x"), b"x").is_err());
    }
}
