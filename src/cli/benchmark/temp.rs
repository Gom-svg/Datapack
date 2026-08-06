use std::path::{Path, PathBuf};

pub(super) fn benchmark_temp_path(input: &Path) -> PathBuf {
    let mut path = benchmark_temp_root(input);
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("datapack-benchmark");
    path.push(format!("{stem}-{}-benchmark.dpack", std::process::id()));
    path
}

pub(super) fn benchmark_restore_path(input: &Path) -> PathBuf {
    let mut path = benchmark_temp_root(input);
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("datapack-benchmark");
    path.push(format!(
        "{stem}-{}-benchmark-restored.tmp",
        std::process::id()
    ));
    path
}

pub(super) fn benchmark_zstd_temp_path(input: &Path) -> PathBuf {
    let mut path = benchmark_temp_root(input);
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("datapack-benchmark");
    path.push(format!(
        "{stem}-{}-benchmark-baseline.zst",
        std::process::id()
    ));
    path
}

pub(super) fn benchmark_chunked_temp_path(input: &Path) -> PathBuf {
    let mut path = benchmark_temp_root(input);
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("datapack-benchmark");
    path.push(format!(
        "{stem}-{}-benchmark-chunked.dpack",
        std::process::id()
    ));
    path
}

pub(super) fn benchmark_chunked_restore_path(input: &Path) -> PathBuf {
    let mut path = benchmark_temp_root(input);
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("datapack-benchmark");
    path.push(format!(
        "{stem}-{}-benchmark-chunked-restored.tmp",
        std::process::id()
    ));
    path
}

pub(super) fn benchmark_chunked_sample_path(input: &Path) -> PathBuf {
    let mut path = benchmark_temp_root(input);
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("datapack-benchmark");
    path.push(format!(
        "{stem}-{}-benchmark-prefix.tmp",
        std::process::id()
    ));
    path
}

fn benchmark_temp_root(input: &Path) -> PathBuf {
    std::env::var_os("DATAPACK_TEMP_DIR")
        .map(PathBuf::from)
        .or_else(|| input.parent().map(Path::to_path_buf))
        .unwrap_or_else(std::env::temp_dir)
}

pub(super) struct BenchmarkTempFiles {
    paths: Vec<PathBuf>,
    keep: bool,
}

impl BenchmarkTempFiles {
    pub(super) fn new(keep: bool, paths: Vec<PathBuf>) -> Self {
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
