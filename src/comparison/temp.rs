use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{DatapackError, Result};

const WORKSPACE_CREATE_ATTEMPTS: usize = 1_000;
static WORKSPACE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Collision-safe temporary artifacts owned by one comparison run.
///
/// Cleanup is deliberately non-recursive. Only the fixed files owned by this
/// workspace are removed before the uniquely created directory itself is
/// removed.
pub(super) struct ComparisonWorkspace {
    directory: PathBuf,
    measured_input: PathBuf,
    datapack_archive: PathBuf,
    zstd_archive: PathBuf,
    datapack_restore: PathBuf,
    zstd_restore: PathBuf,
    finished: bool,
}

impl ComparisonWorkspace {
    pub(super) fn create(input: &Path) -> Result<Self> {
        let parent = workspace_parent(input)?;
        validate_workspace_parent(&parent)?;

        for _ in 0..WORKSPACE_CREATE_ATTEMPTS {
            let sequence = WORKSPACE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let directory = parent.join(format!(
                ".datapack-compare-{}-{sequence}",
                std::process::id()
            ));
            match create_workspace_directory(&directory) {
                Ok(()) => {
                    let measured_input = directory.join(measured_input_name(input));
                    return Ok(Self {
                        datapack_archive: directory.join("datapack.dpack"),
                        zstd_archive: directory.join("standalone.zst"),
                        datapack_restore: directory.join("datapack-restored.bin"),
                        zstd_restore: directory.join("zstd-restored.bin"),
                        directory,
                        measured_input,
                        finished: false,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(workspace_io_error(format!(
                        "could not create comparison workspace in '{}': {error}",
                        parent.display()
                    )))
                }
            }
        }

        Err(workspace_io_error(format!(
            "could not reserve a unique comparison workspace in '{}' after {WORKSPACE_CREATE_ATTEMPTS} attempts",
            parent.display()
        )))
    }

    pub(super) fn measured_input(&self) -> &Path {
        &self.measured_input
    }

    pub(super) fn datapack_archive(&self) -> &Path {
        &self.datapack_archive
    }

    pub(super) fn zstd_archive(&self) -> &Path {
        &self.zstd_archive
    }

    pub(super) fn datapack_restore(&self) -> &Path {
        &self.datapack_restore
    }

    pub(super) fn zstd_restore(&self) -> &Path {
        &self.zstd_restore
    }

    /// Removes every owned artifact and then the workspace directory.
    ///
    /// An unexpected entry prevents directory removal and is reported rather
    /// than removed recursively.
    pub(super) fn finish(&mut self) -> Result<()> {
        if self.finished {
            return Ok(());
        }

        let mut cleanup_errors = Vec::new();
        for path in self.owned_files() {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => cleanup_errors.push(format!("{}: {error}", path.display())),
            }
        }

        match fs::remove_dir(&self.directory) {
            Ok(()) => self.finished = true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => self.finished = true,
            Err(error) => cleanup_errors.push(format!("{}: {error}", self.directory.display())),
        }

        if cleanup_errors.is_empty() {
            Ok(())
        } else {
            Err(workspace_io_error(format!(
                "could not fully clean comparison workspace: {}",
                cleanup_errors.join("; ")
            )))
        }
    }

    fn owned_files(&self) -> [&Path; 5] {
        [
            &self.measured_input,
            &self.datapack_archive,
            &self.zstd_archive,
            &self.datapack_restore,
            &self.zstd_restore,
        ]
    }
}

impl Drop for ComparisonWorkspace {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

fn create_workspace_directory(path: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

fn workspace_parent(input: &Path) -> Result<PathBuf> {
    if let Some(configured) = std::env::var_os("DATAPACK_TEMP_DIR") {
        if configured.is_empty() {
            return Err(workspace_io_error(
                "DATAPACK_TEMP_DIR must not be empty".to_string(),
            ));
        }
        return Ok(PathBuf::from(configured));
    }

    Ok(input
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir))
}

fn validate_workspace_parent(parent: &Path) -> Result<()> {
    let metadata = fs::metadata(parent).map_err(|error| {
        workspace_io_error(format!(
            "comparison workspace parent '{}' is not accessible: {error}",
            parent.display()
        ))
    })?;
    if !metadata.is_dir() {
        return Err(workspace_io_error(format!(
            "comparison workspace parent '{}' is not a directory",
            parent.display()
        )));
    }
    Ok(())
}

fn workspace_io_error(message: String) -> DatapackError {
    DatapackError::Io(std::io::Error::other(message))
}

fn measured_input_name(input: &Path) -> OsString {
    let mut name = OsString::from("measured-input");
    if let Some(extension) = input.extension().filter(|extension| !extension.is_empty()) {
        name.push(".");
        name.push(extension);
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_unique_and_snapshot_retains_input_extension() {
        let parent = tempfile::tempdir().expect("temporary parent");
        let input = parent.path().join("source.data.csv");
        fs::write(&input, b"header\nvalue\n").expect("write input");

        let mut first = ComparisonWorkspace::create(&input).expect("first workspace");
        let mut second = ComparisonWorkspace::create(&input).expect("second workspace");

        assert_ne!(first.directory, second.directory);
        assert_eq!(
            first.measured_input().file_name(),
            Some(std::ffi::OsStr::new("measured-input.csv"))
        );
        assert_eq!(
            first.datapack_archive().file_name(),
            Some(std::ffi::OsStr::new("datapack.dpack"))
        );
        assert_eq!(
            first.zstd_archive().file_name(),
            Some(std::ffi::OsStr::new("standalone.zst"))
        );
        assert_eq!(
            first.datapack_restore().file_name(),
            Some(std::ffi::OsStr::new("datapack-restored.bin"))
        );
        assert_eq!(
            first.zstd_restore().file_name(),
            Some(std::ffi::OsStr::new("zstd-restored.bin"))
        );

        let first_directory = first.directory.clone();
        let second_directory = second.directory.clone();
        first.finish().expect("clean first workspace");
        second.finish().expect("clean second workspace");
        assert!(!first_directory.exists());
        assert!(!second_directory.exists());
    }

    #[test]
    fn finish_removes_owned_files_without_removing_unexpected_entries() {
        let parent = tempfile::tempdir().expect("temporary parent");
        let input = parent.path().join("source.tsv");
        fs::write(&input, b"a\tb\n").expect("write input");
        let mut workspace = ComparisonWorkspace::create(&input).expect("workspace");
        for path in workspace.owned_files() {
            fs::write(path, b"owned").expect("write owned artifact");
        }
        let unexpected = workspace.directory.join("unexpected.txt");
        fs::write(&unexpected, b"do not remove").expect("write unexpected entry");

        assert!(workspace.finish().is_err());
        assert!(unexpected.exists());
        assert!(workspace
            .owned_files()
            .into_iter()
            .all(|path| !path.exists()));

        fs::remove_file(&unexpected).expect("remove test entry");
        workspace.finish().expect("retry workspace cleanup");
        assert!(!workspace.directory.exists());
    }

    #[test]
    fn drop_removes_owned_artifacts_and_workspace() {
        let parent = tempfile::tempdir().expect("temporary parent");
        let input = parent.path().join("source.psv");
        fs::write(&input, b"a|b\n").expect("write input");
        let directory;
        {
            let workspace = ComparisonWorkspace::create(&input).expect("workspace");
            directory = workspace.directory.clone();
            for path in workspace.owned_files() {
                fs::write(path, b"owned").expect("write owned artifact");
            }
        }
        assert!(!directory.exists());
    }
}
