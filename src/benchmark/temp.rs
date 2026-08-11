use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static BENCHMARK_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub(super) struct BenchmarkTempPaths {
    pub(super) datapack: PathBuf,
    pub(super) restore: PathBuf,
    pub(super) zstd: PathBuf,
    pub(super) chunked: PathBuf,
    pub(super) chunked_restore: PathBuf,
    pub(super) chunked_sample: PathBuf,
}

impl BenchmarkTempPaths {
    pub(super) fn new(input: &Path) -> Self {
        let root = benchmark_temp_root(input);
        let stem = input
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("datapack-benchmark");
        let process_id = std::process::id();
        let sequence = BENCHMARK_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let prefix = format!("{stem}-{process_id}-{sequence}");
        Self {
            datapack: root.join(format!("{prefix}-benchmark.dpack")),
            restore: root.join(format!("{prefix}-benchmark-restored.tmp")),
            zstd: root.join(format!("{prefix}-benchmark-baseline.zst")),
            chunked: root.join(format!("{prefix}-benchmark-chunked.dpack")),
            chunked_restore: root.join(format!("{prefix}-benchmark-chunked-restored.tmp")),
            chunked_sample: root.join(format!("{prefix}-benchmark-prefix.tmp")),
        }
    }

    pub(super) fn owned_paths(&self) -> Vec<PathBuf> {
        vec![
            self.datapack.clone(),
            self.restore.clone(),
            self.zstd.clone(),
            self.chunked.clone(),
            self.chunked_restore.clone(),
            self.chunked_sample.clone(),
        ]
    }
}

fn benchmark_temp_root(input: &Path) -> PathBuf {
    std::env::var_os("DATAPACK_TEMP_DIR")
        .map(PathBuf::from)
        .or_else(|| input.parent().map(Path::to_path_buf))
        .unwrap_or_else(std::env::temp_dir)
}

pub(crate) struct BenchmarkTempFiles {
    paths: Vec<PathBuf>,
    keep: bool,
}

impl BenchmarkTempFiles {
    pub(crate) fn new(keep: bool, paths: Vec<PathBuf>) -> Self {
        Self { paths, keep }
    }
}

impl Drop for BenchmarkTempFiles {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        for path in &self.paths {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benchmark_runs_reserve_distinct_same_process_paths() {
        let input = Path::new("example.csv");
        let first = BenchmarkTempPaths::new(input);
        let second = BenchmarkTempPaths::new(input);

        for first_path in first.owned_paths() {
            assert!(!second.owned_paths().contains(&first_path));
        }
    }

    #[test]
    fn one_run_id_is_shared_by_every_reserved_suffix() {
        let paths = BenchmarkTempPaths::new(Path::new("example.csv"));
        let datapack_name = paths.datapack.file_name().unwrap().to_string_lossy();
        let prefix = datapack_name.strip_suffix("-benchmark.dpack").unwrap();

        assert_eq!(
            paths.restore.file_name().unwrap().to_string_lossy(),
            format!("{prefix}-benchmark-restored.tmp")
        );
        assert_eq!(
            paths.zstd.file_name().unwrap().to_string_lossy(),
            format!("{prefix}-benchmark-baseline.zst")
        );
        assert_eq!(
            paths.chunked.file_name().unwrap().to_string_lossy(),
            format!("{prefix}-benchmark-chunked.dpack")
        );
        assert_eq!(
            paths.chunked_restore.file_name().unwrap().to_string_lossy(),
            format!("{prefix}-benchmark-chunked-restored.tmp")
        );
        assert_eq!(
            paths.chunked_sample.file_name().unwrap().to_string_lossy(),
            format!("{prefix}-benchmark-prefix.tmp")
        );
    }
}
