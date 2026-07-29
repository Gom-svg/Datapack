#![forbid(unsafe_code)]

use datapack::cli::{run, Cli};

fn main() {
    let cli = <Cli as clap::Parser>::parse();

    if let Err(err) = run(cli) {
        eprintln!("error: {err}");
        std::process::exit(err.exit_code());
    }
}
