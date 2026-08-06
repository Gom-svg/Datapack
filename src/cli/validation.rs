use std::path::{Path, PathBuf};

use crate::error::{DatapackError, Result};

pub(super) fn validate_input_output_paths(input: &Path, output: &Path) -> Result<()> {
    let input_metadata = std::fs::metadata(input).map_err(|error| {
        DatapackError::InvalidFormat(format!(
            "input path '{}' was not found or is not readable: {error}",
            input.display()
        ))
    })?;
    if !input_metadata.is_file() {
        return Err(DatapackError::InvalidFormat(format!(
            "input path '{}' is not a regular file",
            input.display()
        )));
    }

    let input = normalized_cli_path(input)?;
    let output = normalized_cli_path(output)?;
    let same = if cfg!(windows) {
        input
            .to_string_lossy()
            .eq_ignore_ascii_case(&output.to_string_lossy())
    } else {
        input == output
    };
    if same {
        return Err(DatapackError::InvalidFormat(format!(
            "output path '{}' must differ from input/archive path '{}'",
            output.display(),
            input.display()
        )));
    }

    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent_metadata = std::fs::metadata(parent).map_err(|error| {
        DatapackError::InvalidFormat(format!(
            "output parent directory '{}' does not exist or is inaccessible: {error}",
            parent.display()
        ))
    })?;
    if !parent_metadata.is_dir() {
        return Err(DatapackError::InvalidFormat(format!(
            "output parent '{}' is not a directory",
            parent.display()
        )));
    }
    Ok(())
}

pub(super) fn validate_output_overwrite_policy(path: &Path, force: bool) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.file_type().is_file() {
                return Err(DatapackError::InvalidFormat(format!(
                    "output path '{}' exists but is not a regular file",
                    path.display()
                )));
            }
            if !force {
                return Err(DatapackError::InvalidFormat(format!(
                    "output file '{}' already exists; pass --force to replace it",
                    path.display()
                )));
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DatapackError::InvalidFormat(format!(
            "could not inspect output path '{}': {error}",
            path.display()
        ))),
    }
}

pub(super) fn validate_nonzero_megabyte_limit(flag: &str, mb: u64) -> Result<()> {
    if mb == 0 {
        return Err(DatapackError::InvalidFormat(format!(
            "{flag} must be greater than zero"
        )));
    }
    let _ = megabytes_to_bytes(flag, mb)?;
    Ok(())
}

pub(super) fn optional_megabytes_to_bytes(flag: &str, value: Option<u64>) -> Result<Option<u64>> {
    value.map(|mb| megabytes_to_bytes(flag, mb)).transpose()
}

pub(super) fn megabytes_to_bytes(flag: &str, mb: u64) -> Result<u64> {
    mb.checked_mul(1024 * 1024)
        .ok_or_else(|| DatapackError::InvalidFormat(format!("{flag} is too large")))
}

pub(super) fn operation_failed(
    operation: &'static str,
    input: &Path,
    output: &Path,
    output_existed: bool,
    error: DatapackError,
) -> DatapackError {
    DatapackError::OperationFailed {
        operation,
        input: input.to_path_buf(),
        output: output.to_path_buf(),
        reason: error.to_string(),
        output_status: if output_existed {
            "previous output preserved"
        } else {
            "no final output was committed"
        },
    }
}

pub(super) fn normalized_cli_path(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return Ok(std::fs::canonicalize(path)?);
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let file_name = absolute.file_name().ok_or_else(|| {
        DatapackError::InvalidFormat(format!("invalid output path: {}", path.display()))
    })?;
    let parent = absolute.parent().unwrap_or_else(|| Path::new("."));
    let parent = if parent.exists() {
        std::fs::canonicalize(parent)?
    } else {
        parent.to_path_buf()
    };
    Ok(parent.join(file_name))
}
