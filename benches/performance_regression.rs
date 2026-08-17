mod support;

use std::error::Error;
use std::path::PathBuf;

use support::{HarnessResult, Preset};

#[derive(Debug)]
struct Options {
    preset: Preset,
    runs: usize,
    work_dir: Option<PathBuf>,
    output: Option<PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            preset: Preset::Smoke,
            runs: 1,
            work_dir: None,
            output: None,
        }
    }
}

impl Options {
    fn parse() -> HarnessResult<Self> {
        let mut options = Self::default();
        let mut arguments = std::env::args().skip(1);
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--bench" => {}
                "--preset" => {
                    let value = arguments.next().ok_or("--preset requires a value")?;
                    options.preset = Preset::parse(&value)?;
                }
                "--runs" => {
                    let value = arguments.next().ok_or("--runs requires a value")?;
                    options.runs = value.parse()?;
                }
                "--work-dir" => {
                    options.work_dir = Some(PathBuf::from(
                        arguments.next().ok_or("--work-dir requires a value")?,
                    ));
                }
                "--output" => {
                    options.output = Some(PathBuf::from(
                        arguments.next().ok_or("--output requires a value")?,
                    ));
                }
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                unknown => return Err(format!("unknown argument {unknown:?}").into()),
            }
        }
        Ok(options)
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let options = Options::parse()?;
    let temporary;
    let workspace = if let Some(root) = options.work_dir.as_deref() {
        temporary = tempfile::Builder::new()
            .prefix("datapack-performance-")
            .tempdir_in(root)?;
        temporary.path()
    } else {
        temporary = tempfile::Builder::new()
            .prefix("datapack-performance-")
            .tempdir()?;
        temporary.path()
    };

    let report = support::run_suite(
        options.preset,
        options.runs,
        workspace,
        options.output.as_deref(),
    )?;
    let json = serde_json::to_string_pretty(&report)?;
    if let Some(output) = &options.output {
        std::fs::write(output, format!("{json}\n"))?;
    }
    println!("{json}");
    Ok(())
}

fn print_help() {
    println!(
        "DataPack performance/regression harness\n\n\
         Usage: cargo bench --bench performance_regression -- [OPTIONS]\n\n\
         Options:\n\
           --preset <smoke|representative>  Deterministic workload sizes [default: smoke]\n\
           --runs <N>                       Runs per scenario, 1..=25 [default: 1]\n\
           --work-dir <PATH>                Parent for isolated temporary artifacts\n\
           --output <PATH>                  Also write the JSON report to this path\n\
           -h, --help                       Show this help\n\n\
         Correctness failures return non-zero. Wall-clock timings are observational only."
    );
}
