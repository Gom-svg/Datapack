use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::compression::zstd_backend;
use crate::error::{DatapackError, Result};
use crate::storage::output::TempOutput;
use crate::storage::MAGIC;

pub const CHUNKED_VERSION: u16 = 2;
pub const DEFAULT_CHUNK_SIZE_MB: u64 = 64;
pub const V2_HEADER_LEN: usize = 64;
pub const V2_CHUNK_ENTRY_LEN: usize = 80;
pub const MAX_CHUNKS_HARD: u64 = 1_000_000;
pub const MAX_THREAD_COUNT: usize = 256;
pub const MAX_IN_FLIGHT_CHUNKS: usize = 4_096;
pub const MAX_CHUNK_SIZE_MB: u64 = 4_096;

const RAW_ZSTD_MODE: u8 = 1;
const IO_BUFFER_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkedBackend {
    ChunkedRawZstd,
    ZstdMtExperimental,
}

impl ChunkedBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ChunkedRawZstd => "chunked-raw-zstd",
            Self::ZstdMtExperimental => "zstd-mt-experimental",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ChunkedCompressOptions {
    pub chunk_size_bytes: usize,
    pub threads: usize,
    pub max_in_flight_chunks: usize,
    pub backend: ChunkedBackend,
    pub adaptive_level: bool,
    pub profile: bool,
    pub force: bool,
    pub keep_temp: bool,
}

impl ChunkedCompressOptions {
    pub fn new(
        chunk_size_bytes: usize,
        threads: usize,
        adaptive_level: bool,
        profile: bool,
    ) -> Result<Self> {
        Self::new_with_max_in_flight(
            chunk_size_bytes,
            threads,
            default_max_in_flight_chunks(threads),
            adaptive_level,
            profile,
        )
    }

    pub fn new_with_max_in_flight(
        chunk_size_bytes: usize,
        threads: usize,
        max_in_flight_chunks: usize,
        adaptive_level: bool,
        profile: bool,
    ) -> Result<Self> {
        if chunk_size_bytes == 0 {
            return Err(DatapackError::InvalidFormat(
                "chunk size must be greater than zero".to_string(),
            ));
        }
        if threads == 0 {
            return Err(DatapackError::InvalidFormat(
                "thread count must be greater than zero".to_string(),
            ));
        }
        if threads > MAX_THREAD_COUNT {
            return Err(DatapackError::InvalidFormat(format!(
                "--threads must not exceed {MAX_THREAD_COUNT}"
            )));
        }
        if max_in_flight_chunks == 0 {
            return Err(DatapackError::InvalidFormat(
                "max in-flight chunks must be greater than zero".to_string(),
            ));
        }
        if max_in_flight_chunks > MAX_IN_FLIGHT_CHUNKS {
            return Err(DatapackError::InvalidFormat(format!(
                "--max-in-flight-chunks must not exceed {MAX_IN_FLIGHT_CHUNKS}"
            )));
        }
        let chunk_size_u64 = u64::try_from(chunk_size_bytes).map_err(|_| {
            DatapackError::InvalidFormat(
                "--chunk-size-mb is too large for this platform".to_string(),
            )
        })?;
        let maximum_chunk_bytes = MAX_CHUNK_SIZE_MB.checked_mul(1024 * 1024).ok_or_else(|| {
            DatapackError::InvalidFormat("chunk-size ceiling overflow".to_string())
        })?;
        if chunk_size_u64 > maximum_chunk_bytes {
            return Err(DatapackError::InvalidFormat(format!(
                "--chunk-size-mb must not exceed {MAX_CHUNK_SIZE_MB}"
            )));
        }
        Ok(Self {
            chunk_size_bytes,
            threads,
            max_in_flight_chunks,
            backend: ChunkedBackend::ChunkedRawZstd,
            adaptive_level,
            profile,
            force: false,
            keep_temp: false,
        })
    }

    pub fn with_backend(mut self, backend: ChunkedBackend) -> Result<Self> {
        if backend == ChunkedBackend::ZstdMtExperimental {
            u32::try_from(self.threads).map_err(|_| {
                DatapackError::InvalidFormat(
                    "native zstd thread count exceeds the supported maximum".to_string(),
                )
            })?;
        }
        self.backend = backend;
        Ok(self)
    }
}

impl Default for ChunkedCompressOptions {
    fn default() -> Self {
        Self {
            chunk_size_bytes: (DEFAULT_CHUNK_SIZE_MB * 1024 * 1024) as usize,
            threads: default_thread_count(),
            max_in_flight_chunks: default_max_in_flight_chunks(default_thread_count()),
            backend: ChunkedBackend::ChunkedRawZstd,
            adaptive_level: false,
            profile: false,
            force: false,
            keep_temp: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ChunkedDecompressOptions {
    pub verify: bool,
    pub max_output_bytes: Option<u64>,
    pub max_chunks: Option<u64>,
    pub max_memory_bytes: Option<u64>,
    pub force: bool,
    pub keep_temp: bool,
}

impl Default for ChunkedDecompressOptions {
    fn default() -> Self {
        Self {
            verify: true,
            max_output_bytes: None,
            max_chunks: None,
            max_memory_bytes: None,
            force: false,
            keep_temp: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct V2ArchiveLimits {
    pub max_output_bytes: Option<u64>,
    pub max_chunks: Option<u64>,
    pub max_memory_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum V2ValidationStage {
    ChunkPayload,
    ChunkLength,
    ChunkHash,
    RestoredTotal,
    GlobalHash,
}

#[derive(Debug)]
pub(crate) struct V2ValidationError {
    pub(crate) stage: V2ValidationStage,
    pub(crate) source: DatapackError,
}

impl V2ValidationError {
    fn new(stage: V2ValidationStage, source: DatapackError) -> Self {
        Self { stage, source }
    }

    pub(crate) fn into_datapack_error(self) -> DatapackError {
        self.source
    }
}

#[derive(Debug, Clone)]
pub struct ChunkedArchiveInfo {
    pub original_size_bytes: u64,
    pub global_sha256: [u8; 32],
    pub chunk_count: u64,
    pub chunk_size_target: u64,
    pub archive_mode: u8,
    pub chunks: Vec<ChunkEntry>,
}

#[derive(Debug, Clone)]
pub struct ChunkEntry {
    pub chunk_id: u64,
    pub original_offset: u64,
    pub original_size: u64,
    pub compressed_offset: u64,
    pub compressed_size: u64,
    pub compression_mode: u8,
    pub zstd_level: i32,
    pub chunk_sha256: [u8; 32],
}

#[derive(Debug, Clone)]
pub struct ChunkedStats {
    pub original_size_bytes: u64,
    pub archive_size_bytes: u64,
    pub chunk_count: u64,
    pub chunk_size_target: u64,
    pub profile: ChunkedProfile,
}

#[derive(Debug, Clone, Default)]
pub struct ChunkedProfile {
    pub backend: String,
    pub threads: usize,
    pub max_in_flight_chunks: usize,
    pub adaptive_level: bool,
    pub verify_enabled: bool,
    pub read_ms: u64,
    pub hash_ms: u64,
    pub compress_ms: u64,
    pub decompress_ms: u64,
    pub write_ms: u64,
    pub verify_ms: u64,
    pub table_write_ms: u64,
    pub total_elapsed_ms: u64,
    pub average_chunk_transform_ms: f64,
    pub fastest_chunk_ms: f64,
    pub slowest_chunk_ms: f64,
    pub average_compressed_chunk_size: u64,
    pub zstd_level_distribution: Vec<(i32, u64)>,
}

/// Raw progress facts emitted by the v2 chunked storage pipeline.
///
/// This stays crate-private so the application API can translate storage
/// details into its stable progress contract without exposing wire-layer
/// implementation details. Callbacks are invoked only by the caller thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChunkedProgress {
    Compression {
        chunks_read: u64,
        chunks_compressed: u64,
        chunks_written: u64,
        total_chunks: u64,
        bytes_read: u64,
        bytes_compressed: u64,
        bytes_written: u64,
        total_bytes: u64,
        archive_bytes_written: u64,
        max_in_flight_chunks: usize,
        zstd_level: i32,
    },
    Decompression {
        chunks_completed: u64,
        total_chunks: u64,
        bytes_written: u64,
        total_bytes: u64,
    },
}

struct ChunkWork {
    chunk_id: u64,
    original_offset: u64,
    bytes: Vec<u8>,
    adaptive_level: bool,
}

struct CompressedChunk {
    chunk_id: u64,
    original_offset: u64,
    original_size: u64,
    zstd_level: i32,
    chunk_sha256: [u8; 32],
    compressed: Vec<u8>,
    hash_duration: Duration,
    compress_duration: Duration,
}

struct ReaderSummary {
    global_sha256: [u8; 32],
    read_duration: Duration,
    hash_duration: Duration,
}

enum PipelineMessage {
    Chunk(Result<CompressedChunk>),
    Reader(Result<ReaderSummary>),
}

#[derive(Default)]
struct PipelineCounters {
    chunks_read: AtomicU64,
    chunks_compressed: AtomicU64,
    chunks_written: AtomicU64,
    bytes_read: AtomicU64,
    bytes_compressed: AtomicU64,
    bytes_written: AtomicU64,
}

#[derive(Default)]
struct CompressionAccumulator {
    hash_duration: Duration,
    compress_duration: Duration,
    write_duration: Duration,
    chunk_transform_durations: Vec<Duration>,
    compressed_bytes: u64,
    zstd_levels: HashMap<i32, u64>,
}

pub fn default_thread_count() -> usize {
    std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(1)
        .clamp(1, MAX_THREAD_COUNT)
}

pub fn default_max_in_flight_chunks(threads: usize) -> usize {
    threads.saturating_mul(2).max(4)
}

pub fn chunk_size_mb_to_bytes(chunk_size_mb: u64) -> Result<usize> {
    if chunk_size_mb == 0 {
        return Err(DatapackError::InvalidFormat(
            "--chunk-size-mb must be greater than zero".to_string(),
        ));
    }
    if chunk_size_mb > MAX_CHUNK_SIZE_MB {
        return Err(DatapackError::InvalidFormat(format!(
            "--chunk-size-mb must not exceed {MAX_CHUNK_SIZE_MB}"
        )));
    }
    let bytes = chunk_size_mb
        .checked_mul(1024)
        .and_then(|value| value.checked_mul(1024))
        .ok_or_else(|| DatapackError::InvalidFormat("--chunk-size-mb is too large".to_string()))?;
    usize::try_from(bytes)
        .map_err(|_| DatapackError::InvalidFormat("--chunk-size-mb is too large".to_string()))
}

pub fn encode_raw_zstd_chunked_file(
    input_path: &Path,
    output_path: &Path,
    options: ChunkedCompressOptions,
) -> Result<ChunkedStats> {
    encode_raw_zstd_chunked_file_with_progress(input_path, output_path, options, &mut |_| {})
        .map(|(stats, _)| stats)
}

pub(crate) fn encode_raw_zstd_chunked_file_with_progress(
    input_path: &Path,
    output_path: &Path,
    options: ChunkedCompressOptions,
    progress: &mut dyn FnMut(ChunkedProgress),
) -> Result<(ChunkedStats, Option<String>)> {
    ensure_distinct_paths(
        input_path,
        output_path,
        "compression output must differ from input",
    )?;
    let total_started = Instant::now();
    let original_size = std::fs::metadata(input_path)?.len();
    let chunk_size = u64::try_from(options.chunk_size_bytes)
        .map_err(|_| DatapackError::InvalidFormat("chunk size exceeds u64 capacity".to_string()))?;
    let chunk_count = if original_size == 0 {
        0
    } else {
        original_size.div_ceil(chunk_size)
    };
    let chunk_count_usize = usize::try_from(chunk_count).map_err(|_| {
        DatapackError::InvalidFormat("chunk count exceeds platform capacity".to_string())
    })?;

    let mut chunks = expected_chunk_entries(original_size, chunk_size, chunk_count_usize)?;
    let input_file = File::open(input_path)?;
    let (mut temp_guard, temp_file) = TempOutput::create(output_path, options.keep_temp)?;
    let mut output = BufWriter::with_capacity(IO_BUFFER_BYTES, temp_file);
    let table_started = Instant::now();
    write_v2_header_and_table(&mut output, original_size, [0u8; 32], chunk_size, &chunks)?;
    let mut table_write_duration = table_started.elapsed();

    let counters = Arc::new(PipelineCounters::default());
    let (reader_summary, accumulator) = run_compression_pipeline(
        input_file,
        &mut output,
        &mut chunks,
        original_size,
        chunk_size,
        chunk_count,
        options,
        Arc::clone(&counters),
        progress,
    )?;

    output.flush()?;
    output.seek(SeekFrom::Start(0))?;
    let table_started = Instant::now();
    write_v2_header_and_table(
        &mut output,
        original_size,
        reader_summary.global_sha256,
        chunk_size,
        &chunks,
    )?;
    output.flush()?;
    table_write_duration += table_started.elapsed();
    let archive_size = output.get_ref().metadata()?.len();
    drop(output);
    let cleanup_warning = temp_guard.commit_with_cleanup_warning(output_path, options.force)?;

    let mut zstd_level_distribution: Vec<_> = accumulator.zstd_levels.into_iter().collect();
    zstd_level_distribution.sort_by_key(|(level, _)| *level);
    let chunk_total = accumulator.chunk_transform_durations.len() as f64;
    let average_chunk_transform_ms = if chunk_total == 0.0 {
        0.0
    } else {
        accumulator
            .chunk_transform_durations
            .iter()
            .map(duration_ms_f64)
            .sum::<f64>()
            / chunk_total
    };
    let fastest_chunk_ms = accumulator
        .chunk_transform_durations
        .iter()
        .map(duration_ms_f64)
        .reduce(f64::min)
        .unwrap_or(0.0);
    let slowest_chunk_ms = accumulator
        .chunk_transform_durations
        .iter()
        .map(duration_ms_f64)
        .reduce(f64::max)
        .unwrap_or(0.0);

    Ok((
        ChunkedStats {
            original_size_bytes: original_size,
            archive_size_bytes: archive_size,
            chunk_count,
            chunk_size_target: chunk_size,
            profile: ChunkedProfile {
                backend: options.backend.as_str().to_string(),
                threads: options.threads,
                max_in_flight_chunks: options.max_in_flight_chunks,
                adaptive_level: options.adaptive_level,
                read_ms: duration_ms_u64(reader_summary.read_duration),
                hash_ms: duration_ms_u64(reader_summary.hash_duration + accumulator.hash_duration),
                compress_ms: duration_ms_u64(accumulator.compress_duration),
                write_ms: duration_ms_u64(accumulator.write_duration),
                table_write_ms: duration_ms_u64(table_write_duration),
                total_elapsed_ms: duration_ms_u64(total_started.elapsed()),
                average_chunk_transform_ms,
                fastest_chunk_ms,
                slowest_chunk_ms,
                average_compressed_chunk_size: accumulator
                    .compressed_bytes
                    .checked_div(chunk_count)
                    .unwrap_or(0),
                zstd_level_distribution,
                ..ChunkedProfile::default()
            },
        },
        cleanup_warning,
    ))
}

#[allow(clippy::too_many_arguments)]
fn run_compression_pipeline<W: Write + Seek>(
    input_file: File,
    output: &mut W,
    chunks: &mut [ChunkEntry],
    original_size: u64,
    chunk_size: u64,
    chunk_count: u64,
    options: ChunkedCompressOptions,
    counters: Arc<PipelineCounters>,
    progress: &mut dyn FnMut(ChunkedProgress),
) -> Result<(ReaderSummary, CompressionAccumulator)> {
    std::thread::scope(|scope| {
        let cancelled = Arc::new(AtomicBool::new(false));
        let (permit_sender, permit_receiver) =
            mpsc::sync_channel::<()>(options.max_in_flight_chunks);
        for _ in 0..options.max_in_flight_chunks {
            permit_sender.send(()).map_err(|_| {
                DatapackError::InvalidFormat("failed to initialize pipeline permits".to_string())
            })?;
        }
        let (work_sender, work_receiver) =
            mpsc::sync_channel::<ChunkWork>(options.max_in_flight_chunks);
        let work_receiver = Arc::new(Mutex::new(work_receiver));
        let (result_sender, result_receiver) =
            mpsc::sync_channel::<PipelineMessage>(options.max_in_flight_chunks);

        let reader_cancelled = Arc::clone(&cancelled);
        let reader_counters = Arc::clone(&counters);
        let reader_results = result_sender.clone();
        let reader_handle = scope.spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(|| {
                read_chunks(
                    input_file,
                    original_size,
                    chunk_size,
                    chunk_count,
                    options.adaptive_level,
                    permit_receiver,
                    work_sender,
                    reader_cancelled,
                    reader_counters,
                )
            }))
            .unwrap_or_else(|_| {
                Err(DatapackError::InvalidFormat(
                    "chunk reader thread panicked".to_string(),
                ))
            });
            let _ = reader_results.send(PipelineMessage::Reader(result));
        });

        let mut worker_handles = Vec::with_capacity(options.threads);
        let outer_worker_count = match options.backend {
            ChunkedBackend::ChunkedRawZstd => options.threads,
            ChunkedBackend::ZstdMtExperimental => 1,
        };
        for _ in 0..outer_worker_count {
            let worker_receiver = Arc::clone(&work_receiver);
            let worker_results = result_sender.clone();
            let worker_cancelled = Arc::clone(&cancelled);
            let worker_counters = Arc::clone(&counters);
            worker_handles.push(scope.spawn(move || {
                worker_loop(
                    worker_receiver,
                    worker_results,
                    worker_cancelled,
                    worker_counters,
                    options.backend,
                    options.threads,
                )
            }));
        }
        drop(work_receiver);
        drop(result_sender);

        let pipeline_result = write_compressed_chunks(
            output,
            chunks,
            original_size,
            chunk_count,
            options,
            &result_receiver,
            &permit_sender,
            &counters,
            progress,
        );

        cancelled.store(true, Ordering::Release);
        drop(permit_sender);
        drop(result_receiver);

        let reader_panicked = reader_handle.join().is_err();
        let mut worker_panicked = false;
        for handle in worker_handles {
            worker_panicked |= handle.join().is_err();
        }
        if reader_panicked {
            return Err(DatapackError::InvalidFormat(
                "chunk reader thread panicked".to_string(),
            ));
        }
        if worker_panicked {
            return Err(DatapackError::InvalidFormat(
                "chunk compression worker panicked".to_string(),
            ));
        }
        pipeline_result
    })
}

#[allow(clippy::too_many_arguments)]
fn read_chunks(
    input_file: File,
    original_size: u64,
    chunk_size: u64,
    chunk_count: u64,
    adaptive_level: bool,
    permit_receiver: mpsc::Receiver<()>,
    work_sender: mpsc::SyncSender<ChunkWork>,
    cancelled: Arc<AtomicBool>,
    counters: Arc<PipelineCounters>,
) -> Result<ReaderSummary> {
    let mut input = BufReader::with_capacity(IO_BUFFER_BYTES, input_file);
    let mut global_hasher = Sha256::new();
    let mut read_duration = Duration::ZERO;
    let mut hash_duration = Duration::ZERO;
    let mut next_offset = 0u64;

    for chunk_id in 0..chunk_count {
        if cancelled.load(Ordering::Acquire) {
            return Err(DatapackError::InvalidFormat(
                "chunk compression pipeline cancelled".to_string(),
            ));
        }
        permit_receiver.recv().map_err(|_| {
            DatapackError::InvalidFormat("chunk compression pipeline cancelled".to_string())
        })?;
        let remaining = original_size.checked_sub(next_offset).ok_or_else(|| {
            DatapackError::InvalidFormat(
                "chunk reader offset exceeds declared input size".to_string(),
            )
        })?;
        let expected_size = usize::try_from(remaining.min(chunk_size)).map_err(|_| {
            DatapackError::InvalidFormat(
                "chunk size exceeds platform capacity while reading".to_string(),
            )
        })?;
        let mut bytes = vec![0u8; expected_size];
        let read_started = Instant::now();
        input.read_exact(&mut bytes)?;
        read_duration += read_started.elapsed();
        let hash_started = Instant::now();
        global_hasher.update(&bytes);
        hash_duration += hash_started.elapsed();

        let work = ChunkWork {
            chunk_id,
            original_offset: next_offset,
            bytes,
            adaptive_level,
        };
        let expected_size_u64 = u64::try_from(expected_size).map_err(|_| {
            DatapackError::InvalidFormat("chunk size exceeds u64 capacity".to_string())
        })?;
        next_offset = next_offset.checked_add(expected_size_u64).ok_or_else(|| {
            DatapackError::InvalidFormat("chunk reader offset overflow".to_string())
        })?;
        work_sender.send(work).map_err(|_| {
            DatapackError::InvalidFormat("chunk compression workers stopped early".to_string())
        })?;
        counters.chunks_read.fetch_add(1, Ordering::Relaxed);
        counters
            .bytes_read
            .fetch_add(expected_size as u64, Ordering::Relaxed);
    }

    Ok(ReaderSummary {
        global_sha256: global_hasher.finalize().into(),
        read_duration,
        hash_duration,
    })
}

fn worker_loop(
    work_receiver: Arc<Mutex<mpsc::Receiver<ChunkWork>>>,
    result_sender: mpsc::SyncSender<PipelineMessage>,
    cancelled: Arc<AtomicBool>,
    counters: Arc<PipelineCounters>,
    backend: ChunkedBackend,
    threads: usize,
) {
    let mut native_mt = if backend == ChunkedBackend::ZstdMtExperimental {
        match zstd_backend::NativeMtCompressor::new(threads) {
            Ok(compressor) => Some(compressor),
            Err(error) => {
                let _ = result_sender.send(PipelineMessage::Chunk(Err(error)));
                cancelled.store(true, Ordering::Release);
                return;
            }
        }
    } else {
        None
    };
    loop {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        let received = match work_receiver.lock() {
            Ok(receiver) => receiver.recv(),
            Err(_) => {
                let _ = result_sender.send(PipelineMessage::Chunk(Err(
                    DatapackError::InvalidFormat("chunk work queue lock was poisoned".to_string()),
                )));
                break;
            }
        };
        let work = match received {
            Ok(work) => work,
            Err(_) => break,
        };
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        let original_size = work.bytes.len() as u64;
        let result = catch_unwind(AssertUnwindSafe(|| {
            compress_chunk(work, native_mt.as_mut())
        }))
        .unwrap_or_else(|_| {
            Err(DatapackError::InvalidFormat(
                "chunk compression worker panicked".to_string(),
            ))
        });
        if result.is_ok() {
            counters.chunks_compressed.fetch_add(1, Ordering::Relaxed);
            counters
                .bytes_compressed
                .fetch_add(original_size, Ordering::Relaxed);
        } else {
            cancelled.store(true, Ordering::Release);
        }
        if result_sender.send(PipelineMessage::Chunk(result)).is_err() {
            break;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn write_compressed_chunks<W: Write + Seek>(
    output: &mut W,
    chunks: &mut [ChunkEntry],
    original_size: u64,
    chunk_count: u64,
    options: ChunkedCompressOptions,
    result_receiver: &mpsc::Receiver<PipelineMessage>,
    permit_sender: &mpsc::SyncSender<()>,
    counters: &PipelineCounters,
    progress: &mut dyn FnMut(ChunkedProgress),
) -> Result<(ReaderSummary, CompressionAccumulator)> {
    let mut reorder_buffer = BTreeMap::new();
    let mut next_chunk_id = 0u64;
    let table_bytes = chunk_count
        .checked_mul(V2_CHUNK_ENTRY_LEN as u64)
        .ok_or_else(|| {
            DatapackError::InvalidFormat("v2 chunk-table size overflow during write".to_string())
        })?;
    let mut next_compressed_offset =
        (V2_HEADER_LEN as u64)
            .checked_add(table_bytes)
            .ok_or_else(|| {
                DatapackError::InvalidFormat("v2 payload offset overflow during write".to_string())
            })?;
    let mut reader_summary = None;
    let mut accumulator = CompressionAccumulator::default();

    while next_chunk_id < chunk_count || reader_summary.is_none() {
        match result_receiver.recv() {
            Ok(PipelineMessage::Reader(Ok(summary))) => reader_summary = Some(summary),
            Ok(PipelineMessage::Reader(Err(error))) => return Err(error),
            Ok(PipelineMessage::Chunk(Err(error))) => return Err(error),
            Ok(PipelineMessage::Chunk(Ok(chunk))) => {
                if chunk.chunk_id >= chunk_count {
                    return Err(DatapackError::InvalidFormat(format!(
                        "worker returned out-of-range chunk {}",
                        chunk.chunk_id
                    )));
                }
                let chunk_id = chunk.chunk_id;
                if reorder_buffer.insert(chunk_id, chunk).is_some() {
                    return Err(DatapackError::InvalidFormat(format!(
                        "worker returned duplicate chunk {chunk_id}"
                    )));
                }

                while let Some(chunk) = reorder_buffer.remove(&next_chunk_id) {
                    let compressed_offset = next_compressed_offset;
                    let write_started = Instant::now();
                    output.write_all(&chunk.compressed)?;
                    accumulator.write_duration += write_started.elapsed();
                    let compressed_size = u64::try_from(chunk.compressed.len()).map_err(|_| {
                        DatapackError::InvalidFormat(
                            "compressed chunk size exceeds u64 capacity".to_string(),
                        )
                    })?;
                    next_compressed_offset = next_compressed_offset
                        .checked_add(compressed_size)
                        .ok_or_else(|| {
                            DatapackError::InvalidFormat(
                                "v2 compressed offsets exceed u64 capacity".to_string(),
                            )
                        })?;
                    let original_chunk_size = chunk.original_size;
                    let chunk_index = usize::try_from(next_chunk_id).map_err(|_| {
                        DatapackError::InvalidFormat(
                            "chunk index exceeds platform capacity during write".to_string(),
                        )
                    })?;
                    let table_entry = chunks.get_mut(chunk_index).ok_or_else(|| {
                        DatapackError::InvalidFormat(format!(
                            "chunk writer produced out-of-range table index {next_chunk_id}"
                        ))
                    })?;
                    *table_entry = ChunkEntry {
                        chunk_id: chunk.chunk_id,
                        original_offset: chunk.original_offset,
                        original_size: original_chunk_size,
                        compressed_offset,
                        compressed_size,
                        compression_mode: RAW_ZSTD_MODE,
                        zstd_level: chunk.zstd_level,
                        chunk_sha256: chunk.chunk_sha256,
                    };
                    accumulator.hash_duration += chunk.hash_duration;
                    accumulator.compress_duration += chunk.compress_duration;
                    accumulator
                        .chunk_transform_durations
                        .push(chunk.hash_duration + chunk.compress_duration);
                    accumulator.compressed_bytes = accumulator
                        .compressed_bytes
                        .checked_add(compressed_size)
                        .ok_or_else(|| {
                            DatapackError::InvalidFormat(
                                "total compressed byte count overflowed u64".to_string(),
                            )
                        })?;
                    *accumulator.zstd_levels.entry(chunk.zstd_level).or_default() += 1;
                    counters.chunks_written.fetch_add(1, Ordering::Relaxed);
                    counters
                        .bytes_written
                        .fetch_add(original_chunk_size, Ordering::Relaxed);
                    let _ = permit_sender.send(());
                    progress(compression_progress(
                        counters,
                        chunk_count,
                        original_size,
                        next_compressed_offset,
                        options.max_in_flight_chunks,
                        chunk.zstd_level,
                    ));
                    next_chunk_id = next_chunk_id.checked_add(1).ok_or_else(|| {
                        DatapackError::InvalidFormat("chunk id overflow during write".to_string())
                    })?;
                }
            }
            Err(_) => {
                return Err(DatapackError::InvalidFormat(
                    "chunk compression pipeline stopped before completion".to_string(),
                ))
            }
        }
    }

    let reader_summary = reader_summary.ok_or_else(|| {
        DatapackError::InvalidFormat(
            "chunk compression pipeline ended without a reader summary".to_string(),
        )
    })?;
    Ok((reader_summary, accumulator))
}

pub fn decode_raw_zstd_chunked_file(
    input_path: &Path,
    output_path: &Path,
    options: ChunkedDecompressOptions,
) -> Result<ChunkedStats> {
    decode_raw_zstd_chunked_file_with_progress(input_path, output_path, options, &mut |_| {})
        .map(|(stats, _)| stats)
}

pub(crate) fn decode_raw_zstd_chunked_file_with_progress(
    input_path: &Path,
    output_path: &Path,
    options: ChunkedDecompressOptions,
    progress: &mut dyn FnMut(ChunkedProgress),
) -> Result<(ChunkedStats, Option<String>)> {
    ensure_distinct_paths(
        input_path,
        output_path,
        "restored output must differ from archive input",
    )?;
    let total_started = Instant::now();
    let mut input = BufReader::with_capacity(IO_BUFFER_BYTES, File::open(input_path)?);
    let archive_info = read_v2_archive_info_with_limits(
        &mut input,
        V2ArchiveLimits {
            max_output_bytes: options.max_output_bytes,
            max_chunks: options.max_chunks,
            max_memory_bytes: options.max_memory_bytes,
        },
    )
    .map_err(|error| {
        DatapackError::InvalidFormat(format!("archive '{}': {error}", input_path.display()))
    })?;
    let archive_size = input.get_ref().metadata()?.len();
    let (mut temp_guard, temp_file) = TempOutput::create(output_path, options.keep_temp)?;
    let mut output = BufWriter::with_capacity(IO_BUFFER_BYTES, temp_file);
    let mut global_hasher = Sha256::new();
    let mut restored_size = 0u64;
    let mut read_duration = Duration::ZERO;
    let mut decompress_duration = Duration::ZERO;
    let mut write_duration = Duration::ZERO;
    let mut verify_duration = Duration::ZERO;
    let mut chunk_durations = Vec::with_capacity(archive_info.chunks.len());

    for chunk in &archive_info.chunks {
        if chunk.compression_mode != RAW_ZSTD_MODE {
            return Err(DatapackError::InvalidFormat(format!(
                "unsupported v2 chunk compression mode {}",
                chunk.compression_mode
            )));
        }
        input.seek(SeekFrom::Start(chunk.compressed_offset))?;
        let compressed_size = usize::try_from(chunk.compressed_size).map_err(|_| {
            DatapackError::InvalidFormat(format!(
                "compressed payload for chunk {} is too large for this platform",
                chunk.chunk_id
            ))
        })?;
        let mut compressed = Vec::new();
        compressed.try_reserve_exact(compressed_size).map_err(|_| {
            DatapackError::InvalidFormat(format!(
                "compressed payload for chunk {} cannot be allocated safely",
                chunk.chunk_id
            ))
        })?;
        compressed.resize(compressed_size, 0);
        let read_started = Instant::now();
        if let Err(error) = input.read_exact(&mut compressed) {
            if error.kind() == std::io::ErrorKind::UnexpectedEof {
                return Err(DatapackError::InvalidFormat(format!(
                    "compressed payload for chunk {} is truncated",
                    chunk.chunk_id
                )));
            }
            return Err(error.into());
        }
        read_duration += read_started.elapsed();
        let decompress_started = Instant::now();
        let restored_limit = usize::try_from(chunk.original_size).map_err(|_| {
            DatapackError::InvalidFormat(format!(
                "declared original size for chunk {} exceeds platform capacity",
                chunk.chunk_id
            ))
        })?;
        // Zstd independently rejects malformed/truncated frames, including
        // when --no-verify is active; SHA-256 remains the identity authority.
        let restored =
            zstd_backend::decompress_with_limit(&compressed, restored_limit).map_err(|error| {
                DatapackError::InvalidFormat(format!(
                    "zstd decompression failed for chunk {}: {error}",
                    chunk.chunk_id
                ))
            })?;
        let chunk_decompress_duration = decompress_started.elapsed();
        decompress_duration += chunk_decompress_duration;
        chunk_durations.push(chunk_decompress_duration);
        let restored_len = u64::try_from(restored.len()).map_err(|_| {
            DatapackError::InvalidFormat(format!(
                "decompressed size for chunk {} exceeds u64 capacity",
                chunk.chunk_id
            ))
        })?;
        if restored_len != chunk.original_size {
            return Err(DatapackError::InvalidFormat(format!(
                "decompressed size mismatch for chunk {}: expected {}, got {}",
                chunk.chunk_id,
                chunk.original_size,
                restored.len()
            )));
        }
        if options.verify {
            let verify_started = Instant::now();
            let chunk_hash: [u8; 32] = Sha256::digest(&restored).into();
            if chunk_hash != chunk.chunk_sha256 {
                return Err(DatapackError::InvalidFormat(format!(
                    "chunk {} SHA-256 mismatch: expected {}, got {}",
                    chunk.chunk_id,
                    hex_digest(&chunk.chunk_sha256),
                    hex_digest(&chunk_hash)
                )));
            }
            // Updating in validated chunk order makes the global hash cover
            // the complete original input byte sequence without separators.
            global_hasher.update(&restored);
            verify_duration += verify_started.elapsed();
        }
        let write_started = Instant::now();
        output.write_all(&restored)?;
        write_duration += write_started.elapsed();
        restored_size = restored_size.checked_add(restored_len).ok_or_else(|| {
            DatapackError::InvalidFormat(
                "restored output size overflowed u64 during decompression".to_string(),
            )
        })?;
        progress(ChunkedProgress::Decompression {
            chunks_completed: chunk.chunk_id.saturating_add(1),
            total_chunks: archive_info.chunk_count,
            bytes_written: restored_size,
            total_bytes: archive_info.original_size_bytes,
        });
    }

    if restored_size != archive_info.original_size_bytes {
        return Err(DatapackError::InvalidFormat(format!(
            "restored size {restored_size} does not match original size {}",
            archive_info.original_size_bytes
        )));
    }
    if options.verify {
        let verify_started = Instant::now();
        let global_hash: [u8; 32] = global_hasher.finalize().into();
        verify_duration += verify_started.elapsed();
        if global_hash != archive_info.global_sha256 {
            return Err(DatapackError::InvalidFormat(format!(
                "global SHA-256 mismatch: expected {}, got {}",
                hex_digest(&archive_info.global_sha256),
                hex_digest(&global_hash)
            )));
        }
    }
    let write_started = Instant::now();
    output.flush()?;
    write_duration += write_started.elapsed();
    drop(output);
    let cleanup_warning = temp_guard.commit_with_cleanup_warning(output_path, options.force)?;

    let chunk_total = chunk_durations.len() as f64;
    let average_chunk_transform_ms = if chunk_total == 0.0 {
        0.0
    } else {
        chunk_durations.iter().map(duration_ms_f64).sum::<f64>() / chunk_total
    };
    let fastest_chunk_ms = chunk_durations
        .iter()
        .map(duration_ms_f64)
        .reduce(f64::min)
        .unwrap_or(0.0);
    let slowest_chunk_ms = chunk_durations
        .iter()
        .map(duration_ms_f64)
        .reduce(f64::max)
        .unwrap_or(0.0);

    Ok((
        ChunkedStats {
            original_size_bytes: archive_info.original_size_bytes,
            archive_size_bytes: archive_size,
            chunk_count: archive_info.chunk_count,
            chunk_size_target: archive_info.chunk_size_target,
            profile: ChunkedProfile {
                backend: "v2-raw-zstd-frame".to_string(),
                verify_enabled: options.verify,
                read_ms: duration_ms_u64(read_duration),
                decompress_ms: duration_ms_u64(decompress_duration),
                write_ms: duration_ms_u64(write_duration),
                verify_ms: duration_ms_u64(verify_duration),
                total_elapsed_ms: duration_ms_u64(total_started.elapsed()),
                average_chunk_transform_ms,
                fastest_chunk_ms,
                slowest_chunk_ms,
                average_compressed_chunk_size: archive_size
                    .checked_div(archive_info.chunk_count)
                    .unwrap_or(0),
                ..ChunkedProfile::default()
            },
        },
        cleanup_warning,
    ))
}

/// Validates every format-provided integrity guarantee of a v2 archive without
/// creating a restored output or a temporary file.
///
/// `archive_info` must come from `read_v2_archive_info_with_limits` on the same
/// reader. Payload memory is then bounded to one declared compressed chunk plus
/// its declared restored size at a time.
// Windows PathBuf layout makes this crate-private typed error exactly 128 bytes.
// Preserve its validation stage/source contract without an additional allocation.
#[cfg_attr(windows, allow(clippy::result_large_err))]
pub(crate) fn validate_raw_zstd_chunked_payload<R: Read + Seek>(
    input: &mut R,
    archive_info: &ChunkedArchiveInfo,
) -> std::result::Result<(), V2ValidationError> {
    let mut global_hasher = Sha256::new();
    let mut restored_size = 0u64;

    for chunk in &archive_info.chunks {
        if chunk.compression_mode != RAW_ZSTD_MODE {
            return Err(V2ValidationError::new(
                V2ValidationStage::ChunkPayload,
                DatapackError::InvalidFormat(format!(
                    "unsupported v2 chunk compression mode {}",
                    chunk.compression_mode
                )),
            ));
        }

        input
            .seek(SeekFrom::Start(chunk.compressed_offset))
            .map_err(|error| {
                V2ValidationError::new(V2ValidationStage::ChunkPayload, error.into())
            })?;
        let compressed_size = usize::try_from(chunk.compressed_size).map_err(|_| {
            V2ValidationError::new(
                V2ValidationStage::ChunkPayload,
                DatapackError::InvalidFormat(format!(
                    "compressed payload for chunk {} is too large for this platform",
                    chunk.chunk_id
                )),
            )
        })?;
        let mut compressed = Vec::new();
        compressed.try_reserve_exact(compressed_size).map_err(|_| {
            V2ValidationError::new(
                V2ValidationStage::ChunkPayload,
                DatapackError::InvalidFormat(format!(
                    "compressed payload for chunk {} cannot be allocated safely",
                    chunk.chunk_id
                )),
            )
        })?;
        compressed.resize(compressed_size, 0);
        if let Err(error) = input.read_exact(&mut compressed) {
            if error.kind() == std::io::ErrorKind::UnexpectedEof {
                return Err(V2ValidationError::new(
                    V2ValidationStage::ChunkPayload,
                    DatapackError::InvalidFormat(format!(
                        "compressed payload for chunk {} is truncated",
                        chunk.chunk_id
                    )),
                ));
            }
            return Err(V2ValidationError::new(
                V2ValidationStage::ChunkPayload,
                error.into(),
            ));
        }

        let restored_limit = usize::try_from(chunk.original_size).map_err(|_| {
            V2ValidationError::new(
                V2ValidationStage::ChunkPayload,
                DatapackError::InvalidFormat(format!(
                    "declared original size for chunk {} exceeds platform capacity",
                    chunk.chunk_id
                )),
            )
        })?;
        let restored =
            zstd_backend::decompress_with_limit(&compressed, restored_limit).map_err(|error| {
                let stage = if error
                    .to_string()
                    .contains("decompressed output exceeds configured limit")
                {
                    V2ValidationStage::ChunkLength
                } else {
                    V2ValidationStage::ChunkPayload
                };
                V2ValidationError::new(
                    stage,
                    DatapackError::InvalidFormat(format!(
                        "zstd decompression failed for chunk {}: {error}",
                        chunk.chunk_id
                    )),
                )
            })?;
        let restored_len = u64::try_from(restored.len()).map_err(|_| {
            V2ValidationError::new(
                V2ValidationStage::ChunkPayload,
                DatapackError::InvalidFormat(format!(
                    "decompressed size for chunk {} exceeds u64 capacity",
                    chunk.chunk_id
                )),
            )
        })?;
        if restored_len != chunk.original_size {
            return Err(V2ValidationError::new(
                V2ValidationStage::ChunkLength,
                DatapackError::InvalidFormat(format!(
                    "decompressed size mismatch for chunk {}: expected {}, got {}",
                    chunk.chunk_id,
                    chunk.original_size,
                    restored.len()
                )),
            ));
        }

        let chunk_hash: [u8; 32] = Sha256::digest(&restored).into();
        if chunk_hash != chunk.chunk_sha256 {
            return Err(V2ValidationError::new(
                V2ValidationStage::ChunkHash,
                DatapackError::InvalidFormat(format!(
                    "chunk {} SHA-256 mismatch: expected {}, got {}",
                    chunk.chunk_id,
                    hex_digest(&chunk.chunk_sha256),
                    hex_digest(&chunk_hash)
                )),
            ));
        }
        global_hasher.update(&restored);
        restored_size = restored_size.checked_add(restored_len).ok_or_else(|| {
            V2ValidationError::new(
                V2ValidationStage::RestoredTotal,
                DatapackError::InvalidFormat(
                    "restored output size overflowed u64 during validation".to_string(),
                ),
            )
        })?;
    }

    if restored_size != archive_info.original_size_bytes {
        return Err(V2ValidationError::new(
            V2ValidationStage::RestoredTotal,
            DatapackError::InvalidFormat(format!(
                "restored size {restored_size} does not match original size {}",
                archive_info.original_size_bytes
            )),
        ));
    }
    let global_hash: [u8; 32] = global_hasher.finalize().into();
    if global_hash != archive_info.global_sha256 {
        return Err(V2ValidationError::new(
            V2ValidationStage::GlobalHash,
            DatapackError::InvalidFormat(format!(
                "global SHA-256 mismatch: expected {}, got {}",
                hex_digest(&archive_info.global_sha256),
                hex_digest(&global_hash)
            )),
        ));
    }

    Ok(())
}

pub fn read_v2_archive_info<R: Read + Seek>(reader: &mut R) -> Result<ChunkedArchiveInfo> {
    read_v2_archive_info_with_limits(reader, V2ArchiveLimits::default())
}

pub fn read_v2_archive_info_with_limits<R: Read + Seek>(
    reader: &mut R,
    limits: V2ArchiveLimits,
) -> Result<ChunkedArchiveInfo> {
    let archive_file_size = reader.seek(SeekFrom::End(0))?;
    if archive_file_size < V2_HEADER_LEN as u64 {
        return Err(DatapackError::InvalidFormat(format!(
            "truncated v2 header: expected {V2_HEADER_LEN} bytes, archive contains {archive_file_size}"
        )));
    }
    let mut header = [0u8; V2_HEADER_LEN];
    reader.seek(SeekFrom::Start(0))?;
    reader.read_exact(&mut header).map_err(|error| {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            DatapackError::InvalidFormat("truncated v2 header".to_string())
        } else {
            error.into()
        }
    })?;
    if &header[..MAGIC.len()] != MAGIC {
        return Err(DatapackError::InvalidFormat("bad magic bytes".to_string()));
    }
    let version = u16::from_le_bytes([header[5], header[6]]);
    if version != CHUNKED_VERSION {
        return Err(DatapackError::InvalidFormat(format!(
            "expected v2 archive, found version {version}"
        )));
    }
    let archive_mode = header[7];
    if archive_mode != RAW_ZSTD_MODE {
        return Err(DatapackError::InvalidFormat(format!(
            "unsupported v2 archive mode {archive_mode}"
        )));
    }
    let original_size_bytes = read_u64(&header, 8)?;
    let mut global_sha256 = [0u8; 32];
    global_sha256.copy_from_slice(&header[16..48]);
    let chunk_count = read_u64(&header, 48)?;
    let chunk_size_target = read_u64(&header, 56)?;
    if chunk_size_target == 0 {
        return Err(DatapackError::InvalidFormat(
            "chunk_size_target must be non-zero".to_string(),
        ));
    }
    let empty_sha256: [u8; 32] = Sha256::digest([]).into();
    if original_size_bytes == 0 && global_sha256 != empty_sha256 {
        return Err(DatapackError::InvalidFormat(
            "archive metadata is invalid: empty archive has an incorrect global SHA-256"
                .to_string(),
        ));
    }
    validate_declared_limits(original_size_bytes, chunk_count, limits)?;
    if chunk_count > MAX_CHUNKS_HARD {
        return Err(DatapackError::InvalidFormat(format!(
            "archive declares {chunk_count} chunks, exceeding the internal safety ceiling of {MAX_CHUNKS_HARD}"
        )));
    }

    let table_bytes = chunk_count
        .checked_mul(V2_CHUNK_ENTRY_LEN as u64)
        .ok_or_else(|| {
            DatapackError::InvalidFormat(
                "invalid v2 archive: chunk table byte size overflow".to_string(),
            )
        })?;
    let data_start = (V2_HEADER_LEN as u64)
        .checked_add(table_bytes)
        .ok_or_else(|| {
            DatapackError::InvalidFormat(
                "invalid v2 archive: chunk table end offset overflow".to_string(),
            )
        })?;
    if let Some(maximum_memory) = limits.max_memory_bytes {
        let table_allocation = table_bytes
            .checked_add(V2_HEADER_LEN as u64)
            .ok_or_else(|| {
                DatapackError::InvalidFormat("memory-estimate arithmetic overflow".to_string())
            })?;
        if table_allocation > maximum_memory {
            return Err(DatapackError::InvalidFormat(format!(
                "v2 header and chunk table require {table_allocation} bytes, exceeding --max-memory-mb limit of {maximum_memory} bytes"
            )));
        }
    }
    if data_start > archive_file_size {
        return Err(DatapackError::InvalidFormat(format!(
            "invalid v2 archive: chunk table extends beyond file length (table ends at {data_start}, file length is {archive_file_size})"
        )));
    }
    let chunk_count_usize = usize::try_from(chunk_count).map_err(|_| {
        DatapackError::InvalidFormat("chunk count exceeds platform capacity".to_string())
    })?;

    let mut chunks = Vec::new();
    chunks.try_reserve_exact(chunk_count_usize).map_err(|_| {
        DatapackError::InvalidFormat(format!(
            "chunk table with {chunk_count} entries cannot be allocated safely"
        ))
    })?;
    for index in 0..chunk_count_usize {
        let mut entry_bytes = [0u8; V2_CHUNK_ENTRY_LEN];
        reader.read_exact(&mut entry_bytes).map_err(|error| {
            if error.kind() == std::io::ErrorKind::UnexpectedEof {
                DatapackError::InvalidFormat(format!("truncated v2 chunk table at entry {index}"))
            } else {
                error.into()
            }
        })?;
        if entry_bytes[77..80] != [0u8; 3] {
            return Err(DatapackError::InvalidFormat(format!(
                "unsupported v2 chunk-table feature bits at entry {index}"
            )));
        }
        let chunk = decode_chunk_entry(&entry_bytes)?;
        let expected_chunk_id = u64::try_from(index).map_err(|_| {
            DatapackError::InvalidFormat("chunk-table index exceeds u64 capacity".to_string())
        })?;
        if chunk.chunk_id != expected_chunk_id {
            return Err(DatapackError::InvalidFormat(format!(
                "chunk table id {} is out of order at index {index}",
                chunk.chunk_id
            )));
        }
        if chunk.compression_mode != RAW_ZSTD_MODE {
            return Err(DatapackError::InvalidFormat(format!(
                "unsupported v2 chunk compression mode {}",
                chunk.compression_mode
            )));
        }
        chunks.push(chunk);
    }
    validate_chunk_table(
        original_size_bytes,
        chunk_size_target,
        data_start,
        archive_file_size,
        &chunks,
    )?;
    validate_memory_limit(table_bytes, &chunks, limits.max_memory_bytes)?;

    Ok(ChunkedArchiveInfo {
        original_size_bytes,
        global_sha256,
        chunk_count,
        chunk_size_target,
        archive_mode,
        chunks,
    })
}

pub fn chunk_hash_table_offset(chunk_index: usize) -> usize {
    chunk_index
        .checked_mul(V2_CHUNK_ENTRY_LEN)
        .and_then(|offset| V2_HEADER_LEN.checked_add(offset))
        .and_then(|offset| offset.checked_add(45))
        .unwrap_or(usize::MAX)
}

fn expected_chunk_entries(
    original_size: u64,
    chunk_size: u64,
    chunk_count: usize,
) -> Result<Vec<ChunkEntry>> {
    let mut chunks = Vec::new();
    chunks.try_reserve_exact(chunk_count).map_err(|_| {
        DatapackError::InvalidFormat(
            "chunk table cannot be allocated safely during compression".to_string(),
        )
    })?;
    for index in 0..chunk_count {
        let index_u64 = u64::try_from(index).map_err(|_| {
            DatapackError::InvalidFormat("chunk index exceeds u64 capacity".to_string())
        })?;
        let original_offset = index_u64.checked_mul(chunk_size).ok_or_else(|| {
            DatapackError::InvalidFormat("original chunk offset overflow".to_string())
        })?;
        let remaining = original_size.checked_sub(original_offset).ok_or_else(|| {
            DatapackError::InvalidFormat(
                "original chunk offset exceeds input size during compression".to_string(),
            )
        })?;
        chunks.push(ChunkEntry {
            chunk_id: index_u64,
            original_offset,
            original_size: remaining.min(chunk_size),
            compressed_offset: 0,
            compressed_size: 0,
            compression_mode: RAW_ZSTD_MODE,
            zstd_level: zstd_backend::DEFAULT_LEVEL,
            chunk_sha256: [0u8; 32],
        });
    }
    Ok(chunks)
}

fn compress_chunk(
    work: ChunkWork,
    native_mt: Option<&mut zstd_backend::NativeMtCompressor>,
) -> Result<CompressedChunk> {
    let hash_started = Instant::now();
    // Frozen v2 integrity rule: each table hash covers the original,
    // uncompressed bytes of exactly this chunk, not its zstd frame.
    let chunk_sha256: [u8; 32] = Sha256::digest(&work.bytes).into();
    let hash_duration = hash_started.elapsed();
    let compress_started = Instant::now();
    let zstd_level = if work.adaptive_level {
        adaptive_zstd_level(&work.bytes)
    } else {
        zstd_backend::DEFAULT_LEVEL
    };
    let compressed = match native_mt {
        Some(compressor) => compressor.compress_with_level(&work.bytes, zstd_level)?,
        None => zstd_backend::compress_with_level(&work.bytes, zstd_level)?,
    };
    let compress_duration = compress_started.elapsed();
    Ok(CompressedChunk {
        chunk_id: work.chunk_id,
        original_offset: work.original_offset,
        original_size: work.bytes.len() as u64,
        zstd_level,
        chunk_sha256,
        compressed,
        hash_duration,
        compress_duration,
    })
}

fn adaptive_zstd_level(bytes: &[u8]) -> i32 {
    let sample_len = bytes.len().min(64 * 1024);
    if sample_len == 0 {
        return zstd_backend::DEFAULT_LEVEL;
    }
    let sample = &bytes[..sample_len];
    let mut counts = [0usize; 256];
    let mut numeric_like = 0usize;
    for &byte in sample {
        counts[byte as usize] += 1;
        if byte.is_ascii_digit()
            || matches!(
                byte,
                b'-' | b'+' | b'.' | b',' | b'\r' | b'\n' | b'\t' | b' '
            )
        {
            numeric_like += 1;
        }
    }
    let unique = counts.iter().filter(|&&count| count > 0).count();
    let max_frequency = counts.iter().copied().max().unwrap_or(0) as f64 / sample_len as f64;
    let numeric_ratio = numeric_like as f64 / sample_len as f64;

    if unique <= 16 || max_frequency >= 0.20 {
        6
    } else if unique > 180 || (numeric_ratio >= 0.85 && unique > 24) {
        1
    } else {
        zstd_backend::DEFAULT_LEVEL
    }
}

fn write_v2_header_and_table<W: Write>(
    writer: &mut W,
    original_size: u64,
    global_sha256: [u8; 32],
    chunk_size_target: u64,
    chunks: &[ChunkEntry],
) -> Result<()> {
    let mut header = [0u8; V2_HEADER_LEN];
    header[..MAGIC.len()].copy_from_slice(MAGIC);
    header[5..7].copy_from_slice(&CHUNKED_VERSION.to_le_bytes());
    header[7] = RAW_ZSTD_MODE;
    header[8..16].copy_from_slice(&original_size.to_le_bytes());
    header[16..48].copy_from_slice(&global_sha256);
    let chunk_count = u64::try_from(chunks.len()).map_err(|_| {
        DatapackError::InvalidFormat("chunk count exceeds u64 capacity".to_string())
    })?;
    header[48..56].copy_from_slice(&chunk_count.to_le_bytes());
    header[56..64].copy_from_slice(&chunk_size_target.to_le_bytes());
    writer.write_all(&header)?;
    for chunk in chunks {
        writer.write_all(&encode_chunk_entry(chunk))?;
    }
    Ok(())
}

fn encode_chunk_entry(chunk: &ChunkEntry) -> [u8; V2_CHUNK_ENTRY_LEN] {
    let mut entry = [0u8; V2_CHUNK_ENTRY_LEN];
    entry[0..8].copy_from_slice(&chunk.chunk_id.to_le_bytes());
    entry[8..16].copy_from_slice(&chunk.original_offset.to_le_bytes());
    entry[16..24].copy_from_slice(&chunk.original_size.to_le_bytes());
    entry[24..32].copy_from_slice(&chunk.compressed_offset.to_le_bytes());
    entry[32..40].copy_from_slice(&chunk.compressed_size.to_le_bytes());
    entry[40] = chunk.compression_mode;
    entry[41..45].copy_from_slice(&chunk.zstd_level.to_le_bytes());
    entry[45..77].copy_from_slice(&chunk.chunk_sha256);
    entry
}

fn decode_chunk_entry(bytes: &[u8; V2_CHUNK_ENTRY_LEN]) -> Result<ChunkEntry> {
    let mut chunk_sha256 = [0u8; 32];
    chunk_sha256.copy_from_slice(&bytes[45..77]);
    Ok(ChunkEntry {
        chunk_id: read_u64(bytes, 0)?,
        original_offset: read_u64(bytes, 8)?,
        original_size: read_u64(bytes, 16)?,
        compressed_offset: read_u64(bytes, 24)?,
        compressed_size: read_u64(bytes, 32)?,
        compression_mode: bytes[40],
        // Informational writer metadata only: decoding is governed by the zstd
        // frame itself. Existing v2 compatibility therefore permits every i32
        // here instead of inventing a new post-freeze validation range.
        zstd_level: i32::from_le_bytes(read_fixed::<4>(bytes, 41)?),
        chunk_sha256,
    })
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(read_fixed::<8>(bytes, offset)?))
}

fn read_fixed<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N]> {
    let end = offset
        .checked_add(N)
        .ok_or_else(|| DatapackError::InvalidFormat("archive field offset overflow".to_string()))?;
    let slice = bytes.get(offset..end).ok_or_else(|| {
        DatapackError::InvalidFormat("archive field extends beyond its fixed record".to_string())
    })?;
    <[u8; N]>::try_from(slice).map_err(|_| {
        DatapackError::InvalidFormat("archive field has an invalid fixed width".to_string())
    })
}

fn validate_declared_limits(
    original_size: u64,
    chunk_count: u64,
    limits: V2ArchiveLimits,
) -> Result<()> {
    if let Some(maximum) = limits.max_output_bytes {
        if original_size > maximum {
            return Err(DatapackError::InvalidFormat(format!(
                "archive declares {original_size} output bytes, exceeding --max-output-mb limit of {maximum} bytes"
            )));
        }
    }
    if let Some(maximum) = limits.max_chunks {
        if chunk_count > maximum {
            return Err(DatapackError::InvalidFormat(format!(
                "archive declares {chunk_count} chunks, exceeding --max-chunks limit of {maximum}"
            )));
        }
    }
    Ok(())
}

fn validate_memory_limit(
    table_bytes: u64,
    chunks: &[ChunkEntry],
    maximum: Option<u64>,
) -> Result<()> {
    let Some(maximum) = maximum else {
        return Ok(());
    };
    let largest_original = chunks
        .iter()
        .map(|chunk| chunk.original_size)
        .max()
        .unwrap_or(0);
    let largest_compressed = chunks
        .iter()
        .map(|chunk| chunk.compressed_size)
        .max()
        .unwrap_or(0);
    let io_buffers = u64::try_from(IO_BUFFER_BYTES)
        .ok()
        .and_then(|bytes| bytes.checked_mul(2))
        .ok_or_else(|| {
            DatapackError::InvalidFormat("memory-estimate arithmetic overflow".to_string())
        })?;
    let estimate = table_bytes
        .checked_add(largest_original)
        .and_then(|value| value.checked_add(largest_compressed))
        .and_then(|value| value.checked_add(io_buffers))
        .ok_or_else(|| {
            DatapackError::InvalidFormat("memory-estimate arithmetic overflow".to_string())
        })?;
    if estimate > maximum {
        return Err(DatapackError::InvalidFormat(format!(
            "estimated decompression memory {estimate} bytes exceeds --max-memory-mb limit of {maximum} bytes"
        )));
    }
    Ok(())
}

fn validate_chunk_table(
    original_size: u64,
    chunk_size_target: u64,
    data_start: u64,
    archive_file_size: u64,
    chunks: &[ChunkEntry],
) -> Result<()> {
    let expected_chunk_count = if original_size == 0 {
        0
    } else {
        original_size.div_ceil(chunk_size_target)
    };
    let actual_chunk_count = u64::try_from(chunks.len()).map_err(|_| {
        DatapackError::InvalidFormat("chunk count exceeds u64 capacity".to_string())
    })?;
    if actual_chunk_count != expected_chunk_count {
        return Err(DatapackError::InvalidFormat(format!(
            "archive metadata is invalid: declared chunk count {actual_chunk_count} does not match original size {original_size} and target chunk size {chunk_size_target} (expected {expected_chunk_count})"
        )));
    }
    if chunks.is_empty() {
        if original_size != 0 {
            return Err(DatapackError::InvalidFormat(
                "archive metadata is invalid: a non-empty output cannot have zero chunks"
                    .to_string(),
            ));
        }
        if data_start != archive_file_size {
            return Err(DatapackError::InvalidFormat(format!(
                "invalid v2 archive: trailing bytes after empty archive header (header ends at {data_start}, file length is {archive_file_size})"
            )));
        }
        return Ok(());
    }

    let mut expected_original_offset = 0u64;
    let mut expected_compressed_offset = data_start;
    for chunk in chunks {
        if chunk.original_size == 0 {
            return Err(DatapackError::InvalidFormat(format!(
                "archive metadata is invalid: chunk {} has original_size == 0",
                chunk.chunk_id
            )));
        }
        if chunk.compressed_size == 0 {
            return Err(DatapackError::InvalidFormat(format!(
                "archive metadata is invalid: chunk {} has compressed_size == 0",
                chunk.chunk_id
            )));
        }
        if chunk.original_size > chunk_size_target {
            return Err(DatapackError::InvalidFormat(format!(
                "archive metadata is invalid: chunk {} original size {} exceeds target chunk size {chunk_size_target}",
                chunk.chunk_id, chunk.original_size
            )));
        }
        let original_end = chunk
            .original_offset
            .checked_add(chunk.original_size)
            .ok_or_else(|| {
                DatapackError::InvalidFormat(format!(
                    "archive metadata is invalid: original offset plus size overflows for chunk {}",
                    chunk.chunk_id
                ))
            })?;
        let compressed_end = chunk
            .compressed_offset
            .checked_add(chunk.compressed_size)
            .ok_or_else(|| {
                DatapackError::InvalidFormat(format!(
                    "archive metadata is invalid: compressed offset plus size overflows for chunk {}",
                    chunk.chunk_id
                ))
            })?;

        if chunk.original_offset != expected_original_offset {
            return Err(DatapackError::InvalidFormat(format!(
                "chunk {} original offset {} does not match expected {}",
                chunk.chunk_id, chunk.original_offset, expected_original_offset
            )));
        }
        expected_original_offset = original_end;
        if chunk.compressed_offset < data_start {
            return Err(DatapackError::InvalidFormat(format!(
                "archive metadata is invalid: chunk {} compressed offset points into the header or chunk table",
                chunk.chunk_id
            )));
        }
        if chunk.compressed_offset > archive_file_size || compressed_end > archive_file_size {
            return Err(DatapackError::InvalidFormat(format!(
                "archive metadata is invalid: compressed range exceeds file length for chunk {} (range ends at {compressed_end}, file length is {archive_file_size})",
                chunk.chunk_id
            )));
        }
        if chunk.compressed_offset < expected_compressed_offset {
            return Err(DatapackError::InvalidFormat(format!(
                "archive metadata is invalid: compressed range for chunk {} overlaps a previous range",
                chunk.chunk_id
            )));
        }
        if chunk.compressed_offset > expected_compressed_offset {
            return Err(DatapackError::InvalidFormat(format!(
                "archive metadata is invalid: unreferenced gap before compressed chunk {}",
                chunk.chunk_id
            )));
        }
        expected_compressed_offset = compressed_end;
    }
    if expected_original_offset != original_size {
        return Err(DatapackError::InvalidFormat(format!(
            "chunk table covers {expected_original_offset} original bytes, expected {original_size}"
        )));
    }
    if expected_compressed_offset != archive_file_size {
        return Err(DatapackError::InvalidFormat(format!(
            "invalid v2 archive: trailing bytes after compressed payloads (payloads end at {expected_compressed_offset}, file length is {archive_file_size})"
        )));
    }
    Ok(())
}

fn hex_digest(bytes: &[u8; 32]) -> String {
    let mut output = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn duration_ms_u64(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

fn duration_ms_f64(duration: &Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn ensure_distinct_paths(input: &Path, output: &Path, message: &str) -> Result<()> {
    let input = normalized_path(input)?;
    let output = normalized_path(output)?;
    let same = if cfg!(windows) {
        input
            .to_string_lossy()
            .eq_ignore_ascii_case(&output.to_string_lossy())
    } else {
        input == output
    };
    if same {
        return Err(DatapackError::InvalidFormat(message.to_string()));
    }
    Ok(())
}

fn normalized_path(path: &Path) -> Result<PathBuf> {
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
    let normalized_parent = if parent.exists() {
        std::fs::canonicalize(parent)?
    } else {
        parent.to_path_buf()
    };
    Ok(normalized_parent.join(file_name))
}

fn compression_progress(
    counters: &PipelineCounters,
    chunk_count: u64,
    total_bytes: u64,
    archive_bytes_written: u64,
    max_in_flight_chunks: usize,
    zstd_level: i32,
) -> ChunkedProgress {
    ChunkedProgress::Compression {
        chunks_read: counters.chunks_read.load(Ordering::Relaxed),
        chunks_compressed: counters.chunks_compressed.load(Ordering::Relaxed),
        chunks_written: counters.chunks_written.load(Ordering::Relaxed),
        total_chunks: chunk_count,
        bytes_read: counters.bytes_read.load(Ordering::Relaxed),
        bytes_compressed: counters.bytes_compressed.load(Ordering::Relaxed),
        bytes_written: counters.bytes_written.load(Ordering::Relaxed),
        total_bytes,
        archive_bytes_written,
        max_in_flight_chunks,
        zstd_level,
    }
}

#[cfg(test)]
mod progress_tests {
    use super::*;

    #[test]
    fn progress_hooks_report_monotonic_chunk_completion_on_the_caller_thread() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let input = directory.path().join("input.bin");
        let archive = directory.path().join("archive.dpack");
        let restored = directory.path().join("restored.bin");
        let original = b"0123456789";
        std::fs::write(&input, original).expect("write input");

        let caller_thread = std::thread::current().id();
        let mut compression_events = Vec::new();
        let options = ChunkedCompressOptions::new_with_max_in_flight(4, 2, 4, false, false)
            .expect("compression options");
        let (stats, cleanup_warning) =
            encode_raw_zstd_chunked_file_with_progress(&input, &archive, options, &mut |event| {
                assert_eq!(std::thread::current().id(), caller_thread);
                compression_events.push(event);
            })
            .expect("encode archive");

        assert_eq!(cleanup_warning, None);
        assert_eq!(stats.chunk_count, 3);
        assert_eq!(compression_events.len(), 3);
        for (index, event) in compression_events.iter().enumerate() {
            let ChunkedProgress::Compression {
                chunks_written,
                total_chunks,
                bytes_written,
                total_bytes,
                archive_bytes_written,
                ..
            } = event
            else {
                panic!("compression emitted a decompression event");
            };
            assert_eq!(*chunks_written, index as u64 + 1);
            assert_eq!(*total_chunks, 3);
            assert_eq!(*bytes_written, ((index + 1) * 4).min(original.len()) as u64);
            assert_eq!(*total_bytes, original.len() as u64);
            assert!(*archive_bytes_written > 0);
        }

        let mut decompression_events = Vec::new();
        let (_, cleanup_warning) = decode_raw_zstd_chunked_file_with_progress(
            &archive,
            &restored,
            ChunkedDecompressOptions::default(),
            &mut |event| {
                assert_eq!(std::thread::current().id(), caller_thread);
                decompression_events.push(event);
            },
        )
        .expect("decode archive");

        assert_eq!(cleanup_warning, None);
        assert_eq!(decompression_events.len(), 3);
        for (index, event) in decompression_events.iter().enumerate() {
            let ChunkedProgress::Decompression {
                chunks_completed,
                total_chunks,
                bytes_written,
                total_bytes,
            } = event
            else {
                panic!("decompression emitted a compression event");
            };
            assert_eq!(*chunks_completed, index as u64 + 1);
            assert_eq!(*total_chunks, 3);
            assert_eq!(*bytes_written, ((index + 1) * 4).min(original.len()) as u64);
            assert_eq!(*total_bytes, original.len() as u64);
        }
        assert_eq!(std::fs::read(&restored).expect("read restored"), original);
    }
}
