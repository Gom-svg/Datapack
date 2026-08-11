use crate::error::{DatapackError, Result};

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
