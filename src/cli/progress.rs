use std::io::IsTerminal;
use std::time::Instant;

use crate::application::{ProgressEvent, ProgressObserver, ProgressPhase, ProgressState};

pub(super) struct TerminalProgressObserver {
    enabled: bool,
    started: Vec<(ProgressPhase, Instant)>,
}

impl TerminalProgressObserver {
    pub(super) fn new() -> Self {
        Self {
            enabled: std::io::stderr().is_terminal(),
            started: Vec::new(),
        }
    }
}

impl ProgressObserver for TerminalProgressObserver {
    fn on_event(&mut self, event: &ProgressEvent) {
        if !self.enabled {
            return;
        }
        if event.is_terminal_success() {
            eprintln!("operation={} status=completed", event.operation.as_str());
            return;
        }
        match event.state {
            ProgressState::Started => {
                if let Some((_, started)) = self
                    .started
                    .iter_mut()
                    .find(|(phase, _)| *phase == event.phase)
                {
                    *started = Instant::now();
                } else {
                    self.started.push((event.phase, Instant::now()));
                }
            }
            ProgressState::Advanced | ProgressState::Completed => {
                let started = self
                    .started
                    .iter()
                    .find(|(phase, _)| *phase == event.phase)
                    .map_or_else(Instant::now, |(_, started)| *started);
                progress_phase(
                    progress_phase_label(event.phase),
                    event.completed_bytes,
                    event.total_bytes,
                    started,
                );
            }
        }
    }
}

fn progress_phase_label(phase: ProgressPhase) -> &'static str {
    match phase {
        ProgressPhase::Analyzing => "analyze sample",
        ProgressPhase::Planning => "planning",
        ProgressPhase::ReadingInput => "read input",
        ProgressPhase::Hashing => "hash",
        ProgressPhase::Compressing => "encode+compress",
        ProgressPhase::WritingArchive => "write archive",
        ProgressPhase::ReadingArchive => "read archive",
        ProgressPhase::Decompressing => "decompress+decode",
        ProgressPhase::WritingOutput => "write restored",
        ProgressPhase::Validating => "validate",
        ProgressPhase::Comparing => "compare",
        ProgressPhase::Benchmarking => "benchmark",
        ProgressPhase::CleaningUp => "cleanup",
        ProgressPhase::Finalizing => "finalize",
    }
}

pub(super) fn progress_phase(
    phase: &str,
    processed_bytes: u64,
    total_bytes: Option<u64>,
    started: Instant,
) {
    let elapsed = started.elapsed().as_secs_f64().max(0.001);
    let mb = processed_bytes as f64 / 1_048_576.0;
    let throughput = mb / elapsed;
    match total_bytes {
        Some(total) => {
            let percent = if total == 0 {
                100.0
            } else {
                (processed_bytes as f64 / total as f64 * 100.0).clamp(0.0, 100.0)
            };
            let bytes_per_second = processed_bytes as f64 / elapsed;
            let eta_seconds = if processed_bytes < total && bytes_per_second > 0.0 {
                (total - processed_bytes) as f64 / bytes_per_second
            } else {
                0.0
            };
            eprintln!(
                "phase={phase} rows=unknown mb={:.2}/{:.2} percent={:.1} elapsed={:.1}s eta={:.1}s throughput={:.2} MB/s",
                mb,
                total as f64 / 1_048_576.0,
                percent,
                elapsed,
                eta_seconds,
                throughput
            );
        }
        None => eprintln!(
            "phase={phase} rows=unknown mb={:.2} elapsed={:.1}s throughput={:.2} MB/s",
            mb, elapsed, throughput
        ),
    }
}
