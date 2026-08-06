use std::fs::File;
use std::io::{BufReader, BufWriter, Cursor, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::analysis;
use crate::error::Result;
use crate::planning::{self, ArchiveMode};
use crate::storage;

use super::archive::{archive_mode_for_payload, encode_best_archive, encode_for_plan};
use super::chunked_options::{
    build_chunked_compress_options, validate_backend_support, validate_chunk_size,
    validate_compression_memory_limit, validate_max_in_flight_chunks, validate_thread_count,
};
use super::profile::{
    duration_ms, elapsed_ms, mb_per_second_u64, print_chunked_profile, print_direct_profile,
    DirectProfile,
};
use super::progress::{
    progress_phase, read_all_buffered_progress, ProgressReader, IO_BUFFER_BYTES,
};
use super::validation::{
    operation_failed, validate_input_output_paths, validate_nonzero_megabyte_limit,
    validate_output_overwrite_policy,
};
use super::{CompressMode, CompressOptions};

pub(super) fn run(input: PathBuf, output: PathBuf, options: CompressOptions) -> Result<()> {
    validate_input_output_paths(&input, &output)?;
    validate_output_overwrite_policy(&output, options.force)?;
    if let Some(chunk_size_mb) = options.chunk_size_mb {
        validate_chunk_size(chunk_size_mb)?;
    }
    if let Some(threads) = options.threads {
        validate_thread_count(threads)?;
    }
    if let Some(max_in_flight) = options.max_in_flight_chunks {
        validate_max_in_flight_chunks(max_in_flight)?;
    }
    if let Some(backend) = options.backend {
        validate_backend_support(backend.storage_backend().as_str())?;
    }
    if let Some(max_memory_mb) = options.max_memory_mb {
        validate_nonzero_megabyte_limit("--max-memory-mb", max_memory_mb)?;
    }
    let output_existed = output.exists();
    compress_command_inner(&input, &output, options)
        .map_err(|error| operation_failed("compression", &input, &output, output_existed, error))
}

fn compress_command_inner(input: &Path, output: &Path, options: CompressOptions) -> Result<()> {
    let total_started = Instant::now();
    if options.uses_chunked() {
        let input_size = std::fs::metadata(input)?.len();
        let mut chunk_options = build_chunked_compress_options(
            options.chunk_size_mb,
            options.threads,
            options.max_in_flight_chunks,
            options.backend,
            options.adaptive_level,
            options.profile,
        )?;
        validate_compression_memory_limit(&chunk_options, options.max_memory_mb)?;
        chunk_options.force = options.force;
        chunk_options.keep_temp = options.keep_temp;
        let transform_started = Instant::now();
        let stats = storage::chunked::encode_raw_zstd_chunked_file(input, output, chunk_options)?;
        let transform_elapsed = transform_started.elapsed();
        if options.profile {
            print_chunked_profile(
                "compress",
                &stats,
                input_size,
                stats.archive_size_bytes,
                elapsed_ms(total_started),
                mb_per_second_u64(input_size, transform_elapsed),
            );
        }
        return Ok(());
    }

    let phase_started = Instant::now();
    let analysis = analysis::analyze_path(input, options.sample_mb)?;
    let planning_ms = elapsed_ms(phase_started);
    let mut plan = analysis.plan.clone();
    for column in planning::apply_dictionary_limits(
        &mut plan,
        &analysis.columns,
        options.max_dictionary_values,
        options.max_dictionary_mb,
    ) {
        eprintln!("Column '{column}' exceeded dictionary limit; switching to Plain.");
    }
    progress_phase("planning", 0, None, phase_started);

    let compares_candidates = options.verify_best
        || (options.mode == CompressMode::Best && plan.estimated_savings_percent < 15.0);
    if plan.archive_mode == ArchiveMode::RawZstd && !compares_candidates {
        let input_size = std::fs::metadata(input)?.len();
        let file = File::open(input)?;
        let reader = BufReader::with_capacity(IO_BUFFER_BYTES, file);
        let mut progress_reader =
            ProgressReader::new(reader, "raw-zstd-compress", Some(input_size));
        let (mut temp_output, temp_file) =
            storage::output::TempOutput::create(output, options.keep_temp)?;
        let mut writer = BufWriter::with_capacity(IO_BUFFER_BYTES, temp_file);
        let transform_started = Instant::now();
        storage::write_raw_zstd_archive_stream(
            input,
            input_size,
            &mut progress_reader,
            &mut writer,
        )?;
        writer.flush()?;
        let output_size = writer.get_ref().metadata()?.len();
        drop(writer);
        temp_output.commit(output, options.force)?;
        progress_reader.finish();
        let transform_elapsed = transform_started.elapsed();
        if options.profile {
            print_direct_profile(&DirectProfile {
                operation: "compress",
                archive_version: 1,
                mode: ArchiveMode::RawZstd.as_str(),
                backend: "raw-zstd-streaming",
                verify_enabled: None,
                input_size_bytes: input_size,
                output_size_bytes: output_size,
                planning_ms: Some(planning_ms),
                read_ms: None,
                transform_ms: duration_ms(transform_elapsed),
                write_ms: None,
                total_ms: elapsed_ms(total_started),
                throughput_mb_per_sec: mb_per_second_u64(input_size, transform_elapsed),
            });
        }
        return Ok(());
    }

    let read_started = Instant::now();
    let bytes = read_all_buffered_progress(input, "read input")?;
    let read_ms = elapsed_ms(read_started);
    let phase_started = Instant::now();
    let (archive, selected_mode) = if options.verify_best {
        let (archive, selected, saved, columnar_error) = encode_best_archive(input, &bytes)?;
        eprintln!(
            "verify-best: selected {}, saved {saved} bytes over alternative.",
            selected.as_str()
        );
        if let Some(error) = columnar_error {
            eprintln!("columnar_candidate_error = {error:?}");
        }
        (archive, selected)
    } else if options.mode == CompressMode::Best && plan.estimated_savings_percent < 15.0 {
        let (archive, selected, _, _) = encode_best_archive(input, &bytes)?;
        (archive, selected)
    } else {
        let archive = encode_for_plan(input, &bytes, plan.archive_mode)?;
        let metadata = storage::read_v1_archive_header(&mut Cursor::new(&archive))?;
        (archive, archive_mode_for_payload(&metadata.payload_kind))
    };
    let transform_elapsed = phase_started.elapsed();
    progress_phase(
        "encode+compress",
        bytes.len() as u64,
        Some(bytes.len() as u64),
        phase_started,
    );

    let (mut temp_output, temp_file) =
        storage::output::TempOutput::create(output, options.keep_temp)?;
    let mut writer = BufWriter::with_capacity(IO_BUFFER_BYTES, temp_file);
    let phase_started = Instant::now();
    writer.write_all(&archive)?;
    writer.flush()?;
    drop(writer);
    temp_output.commit(output, options.force)?;
    let write_ms = elapsed_ms(phase_started);
    progress_phase(
        "write archive",
        archive.len() as u64,
        Some(archive.len() as u64),
        phase_started,
    );
    if options.profile {
        print_direct_profile(&DirectProfile {
            operation: "compress",
            archive_version: 1,
            mode: selected_mode.as_str(),
            backend: "zstd",
            verify_enabled: None,
            input_size_bytes: bytes.len() as u64,
            output_size_bytes: archive.len() as u64,
            planning_ms: Some(planning_ms),
            read_ms: Some(read_ms),
            transform_ms: duration_ms(transform_elapsed),
            write_ms: Some(write_ms),
            total_ms: elapsed_ms(total_started),
            throughput_mb_per_sec: mb_per_second_u64(bytes.len() as u64, transform_elapsed),
        });
    }
    Ok(())
}
