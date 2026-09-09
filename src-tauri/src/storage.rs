//! Same-directory atomic replacement shared by downloads and durable settings.
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

/// Owns a unique file until commit. Failed writes never replace the destination,
/// and dropping the transaction removes only the temporary file it created.
pub(crate) struct AtomicFile {
    temporary: PathBuf,
    target: PathBuf,
    file: Option<File>,
    committed: bool,
}

impl AtomicFile {
    pub fn new(target: &Path) -> io::Result<Self> {
        let parent = target
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target has no parent"))?;
        let name = target
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target has no filename"))?;
        fs::create_dir_all(parent)?;
        for _ in 0..128 {
            let sequence = NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed);
            let mut temporary_name = OsString::from(".");
            temporary_name.push(name);
            temporary_name.push(format!(".shacraft-{}-{sequence}.part", std::process::id()));
            let temporary = parent.join(temporary_name);
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&temporary) {
                Ok(file) => {
                    return Ok(Self {
                        temporary,
                        target: target.to_path_buf(),
                        file: Some(file),
                        committed: false,
                    })
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "cannot allocate a unique temporary file",
        ))
    }

    pub fn writer(&mut self) -> &mut File {
        self.file
            .as_mut()
            .expect("atomic file is open until commit")
    }

    pub fn commit(mut self) -> io::Result<()> {
        self.writer().sync_all()?;
        drop(self.file.take());
        fs::rename(&self.temporary, &self.target)?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for AtomicFile {
    fn drop(&mut self) {
        drop(self.file.take());
        if !self.committed {
            let _ = fs::remove_file(&self.temporary);
        }
    }
}

pub(crate) fn write_atomic(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut output = AtomicFile::new(target)?;
    output.writer().write_all(bytes)?;
    output.commit()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn competing_writers_do_not_share_temporary_files() {
        let root = std::env::temp_dir().join(format!(
            "shacraft-storage-test-{}-{}",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        let target = root.join("settings.json");
        write_atomic(&target, b"original").unwrap();
        let mut first = AtomicFile::new(&target).unwrap();
        let mut second = AtomicFile::new(&target).unwrap();
        assert_ne!(first.temporary, second.temporary);
        first.writer().write_all(b"first").unwrap();
        second.writer().write_all(b"second").unwrap();
        first.commit().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"first");
        drop(second);
        assert_eq!(fs::read(&target).unwrap(), b"first");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn persisted_secrets_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!(
            "shacraft-secret-test-{}-{}",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        let target = root.join("account.json");
        write_atomic(&target, b"token").unwrap();
        assert_eq!(
            target.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(root).unwrap();
    }
}
