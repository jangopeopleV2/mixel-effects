//! Output files: the file system, or an in-memory [`Sink`] chosen per job (the web app, which
//! offers every written file as a download; tests).

use std::io::{BufWriter, Cursor, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[cfg(not(all(target_os = "macos", feature = "mac-app-store")))]
use crate::ExportError;
use crate::{Result, io};

/// Receives each finished output file (path as resolved, complete contents) instead of the file
/// system. Called once per file; image sequences call it from the render threads.
pub type Sink = dyn Fn(&str, Vec<u8>) + Send + Sync;

/// Host-owned resolution of persisted output paths before any file is opened.
pub type Destination = dyn Fn(&Path) -> std::io::Result<PathBuf> + Send + Sync;
/// Notification after successful publication, with Foundation's actual resulting pathname.
pub type Published = dyn Fn(&Path) + Send + Sync;

/// One output file being written.
pub(crate) enum Out<'a> {
    #[cfg(not(all(target_os = "macos", feature = "mac-app-store")))]
    File(BufWriter<std::fs::File>),
    #[cfg(all(target_os = "macos", feature = "mac-app-store"))]
    Atomic {
        file: Box<BufWriter<crate::macos_atomic::StagedFile>>,
        published: Option<&'a Published>,
    },
    Mem {
        path: String,
        cur: Cursor<Vec<u8>>,
        sink: &'a Sink,
    },
}

/// Create an output or in-memory file for `sink`. The App Store output stays in Foundation's
/// replacement directory until commit; other desktop builds create the parent and final file.
pub(crate) fn create<'a>(sink: Option<&'a Sink>, path: &str, destination: Option<&Destination>, published: Option<&'a Published>) -> Result<Out<'a>> {
    match sink {
        Some(sink) => Ok(Out::Mem { path: path.to_string(), cur: Cursor::new(Vec::new()), sink }),
        None => {
            let path = match destination {
                Some(resolve) => resolve(Path::new(path)).map_err(io)?,
                None => PathBuf::from(path),
            };
            #[cfg(all(target_os = "macos", feature = "mac-app-store"))]
            {
                let file = crate::macos_atomic::StagedFile::new(&path).map_err(io)?;
                Ok(Out::Atomic { file: Box::new(BufWriter::new(file)), published })
            }
            #[cfg(not(all(target_os = "macos", feature = "mac-app-store")))]
            {
                let _ = published;
                if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                    std::fs::create_dir_all(dir).map_err(io)?;
                }
                Ok(Out::File(BufWriter::new(std::fs::File::create(path).map_err(io)?)))
            }
        }
    }
}

/// Whether `path` already exists (never for a sink: it has no memory of earlier jobs).
pub(crate) fn exists(sink: Option<&Sink>, path: &str, destination: Option<&Destination>) -> bool {
    sink.is_none()
        && match destination {
            Some(resolve) => resolve(Path::new(path)).is_ok_and(|path| path.exists()),
            None => Path::new(path).exists(),
        }
}

impl Out<'_> {
    /// Flush and close; a sink receives the contents now. Returns the file size.
    pub(crate) fn finish(self) -> Result<u64> {
        match self {
            #[cfg(not(all(target_os = "macos", feature = "mac-app-store")))]
            Out::File(mut w) => {
                w.flush().map_err(io)?;
                let f = w.into_inner().map_err(|e| ExportError::Io(e.to_string()))?;
                Ok(f.metadata().map(|m| m.len()).unwrap_or(0))
            }
            #[cfg(all(target_os = "macos", feature = "mac-app-store"))]
            Out::Atomic { mut file, published } => {
                file.flush().map_err(io)?;
                let file = file.into_inner().map_err(|error| io(error.to_string()))?;
                let len = std::fs::metadata(file.staged_path()).map_err(io)?.len();
                let actual = file.commit().map_err(io)?;
                if let Some(published) = published {
                    published(&actual);
                }
                Ok(len)
            }
            Out::Mem { path, cur, sink } => {
                let data = cur.into_inner();
                let n = data.len() as u64;
                sink(&path, data);
                Ok(n)
            }
        }
    }
}

impl Write for Out<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            #[cfg(not(all(target_os = "macos", feature = "mac-app-store")))]
            Out::File(w) => w.write(buf),
            #[cfg(all(target_os = "macos", feature = "mac-app-store"))]
            Out::Atomic { file, .. } => file.write(buf),
            Out::Mem { cur, .. } => cur.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            #[cfg(not(all(target_os = "macos", feature = "mac-app-store")))]
            Out::File(w) => w.flush(),
            #[cfg(all(target_os = "macos", feature = "mac-app-store"))]
            Out::Atomic { file, .. } => file.flush(),
            Out::Mem { .. } => Ok(()),
        }
    }
}

impl Seek for Out<'_> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        match self {
            #[cfg(not(all(target_os = "macos", feature = "mac-app-store")))]
            Out::File(w) => w.seek(pos),
            #[cfg(all(target_os = "macos", feature = "mac-app-store"))]
            Out::Atomic { file, .. } => file.seek(pos),
            Out::Mem { cur, .. } => cur.seek(pos),
        }
    }
}

/// Atomic output owns only its staging file until commit. A cancellation must never remove
/// a final pathname that may still hold the user's original or a completed earlier frame.
pub(crate) fn remove_cancelled(atomic: bool, paths: &[String], mut remove: impl FnMut(&str) -> std::io::Result<()>) -> Result<()> {
    if atomic {
        return Ok(());
    }
    let mut failure = None;
    for path in paths {
        if let Err(error) = remove(path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            failure.get_or_insert_with(|| crate::ExportError::Io(format!("cannot remove cancelled output {path}: {error}")));
        }
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moved_destination_is_checked_for_skip_existing_and_denied_before_open() {
        let dir = std::env::temp_dir().join(format!("effects-resolved-output-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let actual = dir.join("frame.png");
        std::fs::write(&actual, b"completed previous frame").unwrap();
        let current = actual.clone();
        let destination = move |_: &Path| Ok(current.clone());
        assert!(exists(None, "/saved/frame.png", Some(&destination)));
        let denied = |_: &Path| Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "bookmark denied"));
        assert!(create(None, "/saved/frame.png", Some(&denied), None).is_err());
        assert_eq!(std::fs::read(&actual).unwrap(), b"completed previous frame");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cancelled_atomic_output_preserves_original_and_completed_prefix() {
        let dir = std::env::temp_dir().join(format!("effects-atomic-cancel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let original = dir.join("movie.mov");
        let complete = dir.join("frame_0001.png");
        std::fs::write(&original, b"previous completed movie").unwrap();
        std::fs::write(&complete, b"complete frame").unwrap();
        let paths = vec![original.to_string_lossy().into_owned(), complete.to_string_lossy().into_owned()];
        remove_cancelled(true, &paths, |path| std::fs::remove_file(path)).unwrap();
        assert_eq!(std::fs::read(&original).unwrap(), b"previous completed movie");
        assert_eq!(std::fs::read(&complete).unwrap(), b"complete frame");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ordinary_output_cleanup_attempts_every_owned_path_and_reports_failure() {
        let paths = vec!["one".to_owned(), "two".to_owned(), "three".to_owned()];
        let mut removed = Vec::new();
        let result = remove_cancelled(false, &paths, |path| {
            removed.push(path.to_owned());
            if path == "two" { Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "in use")) } else { Ok(()) }
        });
        assert!(result.unwrap_err().to_string().contains("two"));
        assert_eq!(removed, paths);
    }
}
