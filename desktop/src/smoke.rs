//! Small deterministic executable/adapter boundary. No GUI or terminal parsing.
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::adapter::{CompressionChoice, Controller, Job};
use crate::model::{FileKind, State};

pub const DEMO: &[u8] = include_bytes!("../../tests/fixtures/desktop/demo.csv");

pub fn run(directory: &Path) -> Result<(), String> {
    // A new directory is required; certification never writes over operator files.
    std::fs::create_dir(directory).map_err(|e| e.to_string())?;
    let source = directory.join("demo.csv");
    std::fs::write(&source, DEMO).map_err(|e| e.to_string())?;
    let mut controller = Controller::default();
    complete(
        &mut controller,
        Job::Select {
            path: source.clone(),
            kind: FileKind::Data,
        },
    )?;
    complete(
        &mut controller,
        Job::Analyze {
            input: source.clone(),
        },
    )?;
    if !matches!(controller.state, State::AnalysisReady) {
        return Err("analysis did not finish".into());
    }
    for choice in [CompressionChoice::Recommended, CompressionChoice::Chunked] {
        let label = if choice == CompressionChoice::Recommended {
            "v1"
        } else {
            "v2"
        };
        let archive = directory.join(format!("{label}.dpack"));
        let restored = directory.join(format!("{label}.restored"));
        complete(
            &mut controller,
            Job::Compress {
                input: source.clone(),
                output: archive.clone(),
                choice,
            },
        )?;
        if !matches!(controller.state, State::CompressComplete(_)) {
            return Err("compression did not finish".into());
        }
        complete(
            &mut controller,
            Job::Validate {
                archive: archive.clone(),
                against: Some(source.clone()),
            },
        )?;
        if !matches!(&controller.state, State::ValidationComplete(v) if v.valid && v.source_match.starts_with("Source matches"))
        {
            return Err("source validation failed".into());
        }
        complete(
            &mut controller,
            Job::Decompress {
                archive,
                output: restored.clone(),
            },
        )?;
        if !matches!(controller.state, State::DecompressComplete(_)) {
            return Err("decompression did not finish".into());
        }
        if std::fs::read(restored).map_err(|e| e.to_string())? != DEMO {
            return Err("restored bytes differ".into());
        }
    }
    std::fs::write(
        directory.join("SMOKE-PASSED.txt"),
        "DataPack Desktop 0.1.0\nV1/V2 adapter lifecycle and exact-byte equality: PASS\n",
    )
    .map_err(|e| e.to_string())
}

pub fn complete(controller: &mut Controller, job: Job) -> Result<(), String> {
    controller
        .start(job, Arc::new(|| {}))
        .map_err(|p| p.message)?;
    let deadline = Instant::now() + Duration::from_secs(60);
    while controller.busy() {
        controller.poll();
        if Instant::now() >= deadline {
            controller.cancel();
            return Err("adapter smoke timed out".into());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    if let State::Failed(problem) = &controller.state {
        return Err(format!("{}: {}", problem.code, problem.context));
    }
    Ok(())
}
