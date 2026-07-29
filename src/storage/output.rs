use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{DatapackError, Result};

const COPY_BUFFER_BYTES: usize = 256 * 1024;
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A sibling temporary output that is removed on failure unless preservation
/// was explicitly requested. The final path is never installed by `Drop`.
pub struct TempOutput {
    path: PathBuf,
    keep_on_failure: bool,
    committed: bool,
}

impl TempOutput {
    pub fn create(output_path: &Path, keep_on_failure: bool) -> Result<(Self, File)> {
        let parent = output_parent(output_path)?;
        let file_name = output_path
            .file_name()
            .ok_or_else(|| {
                DatapackError::OutputNotWritable(format!(
                    "output path '{}' has no file name",
                    output_path.display()
                ))
            })?
            .to_string_lossy();

        for _ in 0..100 {
            let sequence = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
            let temp_path = parent.join(format!(
                ".{file_name}.{}.{}.partial",
                std::process::id(),
                sequence
            ));
            match OpenOptions::new()
                .write(true)
                .read(true)
                .create_new(true)
                .open(&temp_path)
            {
                Ok(file) => {
                    return Ok((
                        Self {
                            path: temp_path,
                            keep_on_failure,
                            committed: false,
                        },
                        file,
                    ))
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(DatapackError::OutputNotWritable(format!(
                        "could not create temporary output beside '{}': {error}",
                        output_path.display()
                    )))
                }
            }
        }

        Err(DatapackError::OutputNotWritable(format!(
            "could not create a unique temporary output beside '{}' after 100 attempts",
            output_path.display()
        )))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Atomically installs the verified temporary output when the filesystem
    /// supports sibling renames. A cross-device rename falls back to a
    /// create-new copy, with partial-copy cleanup and previous-output restore.
    pub fn commit(&mut self, output_path: &Path, force: bool) -> Result<()> {
        validate_existing_output(output_path, force)?;

        let backup_path = if output_path.exists() {
            let backup = unique_backup_path(output_path)?;
            std::fs::rename(output_path, &backup).map_err(|error| {
                DatapackError::OutputNotWritable(format!(
                    "could not preserve existing output '{}' before commit: {error}; output status: previous output remains at the final path",
                    output_path.display()
                ))
            })?;
            Some(backup)
        } else {
            None
        };

        let install_result = install_temp(&self.path, output_path);
        if let Err(install_error) = install_result {
            let _ = std::fs::remove_file(output_path);
            let restore_result = match &backup_path {
                Some(backup) => std::fs::rename(backup, output_path),
                None => Ok(()),
            };
            let status = match (backup_path.as_ref(), restore_result) {
                (Some(_), Ok(())) => "previous output restored",
                (Some(backup), Err(ref restore_error)) => {
                    return Err(DatapackError::OutputNotWritable(format!(
                        "could not commit verified temporary output '{}' to '{}': {install_error}; restoring previous output from '{}' also failed: {restore_error}; output status: final path may be absent, previous output remains in the named backup",
                        self.path.display(),
                        output_path.display(),
                        backup.display()
                    )))
                }
                (None, _) => "no final output was created",
            };
            return Err(DatapackError::OutputNotWritable(format!(
                "could not commit verified temporary output '{}' to '{}': {install_error}; output status: {status}",
                self.path.display(),
                output_path.display()
            )));
        }

        self.committed = true;
        if let Some(backup) = backup_path {
            if let Err(error) = std::fs::remove_file(&backup) {
                eprintln!(
                    "warning: committed '{}' but could not remove backup '{}': {error}",
                    output_path.display(),
                    backup.display()
                );
            }
        }
        Ok(())
    }
}

impl Drop for TempOutput {
    fn drop(&mut self) {
        if !self.committed && !self.keep_on_failure {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn output_parent(output_path: &Path) -> Result<&Path> {
    let parent = output_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let metadata = std::fs::metadata(parent).map_err(|error| {
        DatapackError::OutputNotWritable(format!(
            "output parent directory '{}' is not accessible: {error}",
            parent.display()
        ))
    })?;
    if !metadata.is_dir() {
        return Err(DatapackError::OutputNotWritable(format!(
            "output parent '{}' is not a directory",
            parent.display()
        )));
    }
    Ok(parent)
}

fn validate_existing_output(output_path: &Path, force: bool) -> Result<()> {
    match std::fs::symlink_metadata(output_path) {
        Ok(metadata) => {
            if !metadata.file_type().is_file() {
                return Err(DatapackError::OutputNotWritable(format!(
                    "output path '{}' exists but is not a regular file",
                    output_path.display()
                )));
            }
            if !force {
                return Err(DatapackError::OutputNotWritable(format!(
                    "output file '{}' already exists; pass --force to replace it",
                    output_path.display()
                )));
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DatapackError::OutputNotWritable(format!(
            "could not inspect output path '{}': {error}",
            output_path.display()
        ))),
    }
}

fn unique_backup_path(output_path: &Path) -> Result<PathBuf> {
    let parent = output_parent(output_path)?;
    let file_name = output_path
        .file_name()
        .ok_or_else(|| {
            DatapackError::OutputNotWritable(format!(
                "output path '{}' has no file name",
                output_path.display()
            ))
        })?
        .to_string_lossy();
    for _ in 0..100 {
        let sequence = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{file_name}.{}.{}.previous",
            std::process::id(),
            sequence
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(DatapackError::OutputNotWritable(format!(
        "could not reserve a backup path beside '{}'",
        output_path.display()
    )))
}

fn install_temp(temp_path: &Path, output_path: &Path) -> io::Result<()> {
    match std::fs::rename(temp_path, output_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::CrossesDevices => {
            copy_create_new(temp_path, output_path)?;
            std::fs::remove_file(temp_path)
        }
        Err(error) => Err(error),
    }
}

fn copy_create_new(source: &Path, destination: &Path) -> io::Result<()> {
    let source_file = File::open(source)?;
    let destination_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let mut reader = BufReader::with_capacity(COPY_BUFFER_BYTES, source_file);
    let mut writer = BufWriter::with_capacity(COPY_BUFFER_BYTES, destination_file);
    if let Err(error) = io::copy(&mut reader, &mut writer).and_then(|_| writer.flush()) {
        drop(writer);
        let _ = std::fs::remove_file(destination);
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_file_cleaned_up_on_failure() {
        let directory = tempfile::tempdir().expect("temp directory");
        let output = directory.path().join("result.bin");
        let temp_path;
        {
            let (guard, mut file) = TempOutput::create(&output, false).expect("create temp");
            temp_path = guard.path().to_path_buf();
            file.write_all(b"partial").expect("write temp");
        }
        assert!(!temp_path.exists());
        assert!(!output.exists());
    }

    #[test]
    fn keep_temp_preserves_temp_file() {
        let directory = tempfile::tempdir().expect("temp directory");
        let output = directory.path().join("result.bin");
        let temp_path;
        {
            let (guard, mut file) = TempOutput::create(&output, true).expect("create temp");
            temp_path = guard.path().to_path_buf();
            file.write_all(b"partial").expect("write temp");
        }
        assert_eq!(std::fs::read(&temp_path).expect("read temp"), b"partial");
    }

    #[test]
    fn commit_requires_force_and_preserves_existing_file() {
        let directory = tempfile::tempdir().expect("temp directory");
        let output = directory.path().join("result.bin");
        std::fs::write(&output, b"previous").expect("write previous");
        let (mut guard, mut file) = TempOutput::create(&output, false).expect("create temp");
        file.write_all(b"replacement").expect("write temp");
        drop(file);

        let error = guard.commit(&output, false).expect_err("must not clobber");
        assert!(error.to_string().contains("--force"));
        assert_eq!(std::fs::read(&output).expect("read previous"), b"previous");
    }
}
