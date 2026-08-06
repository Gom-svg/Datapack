use std::collections::{BTreeMap, BTreeSet};

use clap::error::ErrorKind;
use clap::{Command, CommandFactory, Parser};
use datapack::cli::Cli;

#[test]
fn structured_cli_surface_is_stable() {
    let root = Cli::command();

    assert_eq!(root.get_name(), "datapack");
    assert_eq!(
        subcommand_names(&root),
        string_set(&[
            "analyze",
            "benchmark",
            "compress",
            "decompress",
            "generate-test-data",
            "tune",
            "validate",
        ])
    );

    let analyze = subcommand(&root, "analyze");
    assert_eq!(
        long_flags(analyze),
        string_set(&["json", "plan", "pretty", "sample-mb"])
    );
    assert_eq!(default_values(analyze), string_map(&[("sample-mb", "64")]));
    assert!(value_enums(analyze).is_empty());

    let pretty_without_json = Cli::try_parse_from(["datapack", "analyze", "input.csv", "--pretty"])
        .expect_err("--pretty must require --json");
    assert_eq!(
        pretty_without_json.kind(),
        ErrorKind::MissingRequiredArgument
    );

    let compress = subcommand(&root, "compress");
    assert_eq!(
        long_flags(compress),
        string_set(&[
            "adaptive-level",
            "backend",
            "chunk-size-mb",
            "chunked",
            "force",
            "keep-temp",
            "max-dictionary-mb",
            "max-dictionary-values",
            "max-in-flight-chunks",
            "max-memory-mb",
            "mode",
            "profile",
            "sample-mb",
            "threads",
            "verify-best",
        ])
    );
    assert_eq!(
        default_values(compress),
        string_map(&[
            ("max-dictionary-mb", "64"),
            ("max-dictionary-values", "65535"),
            ("mode", "fast"),
            ("sample-mb", "64"),
        ])
    );
    assert_eq!(
        value_enums(compress),
        string_map(&[
            ("backend", "chunked-raw-zstd|zstd-mt-experimental"),
            ("mode", "best|fast"),
        ])
    );

    let decompress = subcommand(&root, "decompress");
    assert_eq!(
        long_flags(decompress),
        string_set(&[
            "force",
            "keep-temp",
            "max-chunks",
            "max-memory-mb",
            "max-output-mb",
            "no-verify",
            "profile",
        ])
    );
    assert!(default_values(decompress).is_empty());
    assert!(value_enums(decompress).is_empty());

    let validate = subcommand(&root, "validate");
    assert_eq!(
        long_flags(validate),
        string_set(&[
            "against",
            "json",
            "max-chunks",
            "max-memory-mb",
            "max-output-mb",
            "pretty",
        ])
    );
    assert_eq!(
        default_values(validate),
        string_map(&[("max-memory-mb", "512")])
    );
    assert!(value_enums(validate).is_empty());

    let validate_pretty_without_json =
        Cli::try_parse_from(["datapack", "validate", "archive.dpack", "--pretty"])
            .expect_err("validate --pretty must require --json");
    assert_eq!(
        validate_pretty_without_json.kind(),
        ErrorKind::MissingRequiredArgument
    );

    let generate = subcommand(&root, "generate-test-data");
    assert_eq!(long_flags(generate), string_set(&["rows", "seed"]));
    assert_eq!(default_values(generate), string_map(&[("rows", "10000")]));
    assert!(value_enums(generate).is_empty());

    let tune = subcommand(&root, "tune");
    assert_eq!(
        long_flags(tune),
        string_set(&[
            "adaptive-level",
            "backend",
            "chunk-sizes-mb",
            "force",
            "keep-temp",
            "max-in-flight-chunks",
            "max-input-mb",
            "no-hash",
            "output",
            "profile",
            "runs",
            "skip-roundtrip",
            "threads-list",
        ])
    );
    assert_eq!(
        default_values(tune),
        string_map(&[
            ("backend", "chunked-raw-zstd"),
            ("chunk-sizes-mb", "32,64,128,256"),
            ("runs", "1"),
            ("threads-list", "1,2,4,8,max"),
        ])
    );
    assert_eq!(
        value_enums(tune),
        string_map(&[("backend", "chunked-raw-zstd|zstd-mt-experimental",)])
    );

    let benchmark = subcommand(&root, "benchmark");
    assert_eq!(
        long_flags(benchmark),
        string_set(&[
            "adaptive-level",
            "backend",
            "chunk-size-mb",
            "chunked",
            "estimate-only",
            "json",
            "keep-temp",
            "max-in-flight-chunks",
            "max-input-mb",
            "no-hash",
            "no-roundtrip",
            "no-zstd-baseline",
            "profile",
            "quick",
            "runs",
            "threads",
        ])
    );
    assert_eq!(default_values(benchmark), string_map(&[("runs", "3")]));
    assert_eq!(
        value_enums(benchmark),
        string_map(&[("backend", "chunked-raw-zstd|zstd-mt-experimental",)])
    );

    assert_eq!(
        visible_long_aliases(&root),
        string_map(&[("benchmark.no-roundtrip", "skip-full-roundtrip",)])
    );
}

fn subcommand_names(command: &Command) -> BTreeSet<String> {
    command
        .get_subcommands()
        .map(|subcommand| subcommand.get_name().to_owned())
        .collect()
}

fn subcommand<'a>(command: &'a Command, name: &str) -> &'a Command {
    command
        .get_subcommands()
        .find(|subcommand| subcommand.get_name() == name)
        .unwrap_or_else(|| panic!("missing `{name}` subcommand"))
}

fn long_flags(command: &Command) -> BTreeSet<String> {
    command
        .get_arguments()
        .filter_map(|argument| argument.get_long().map(str::to_owned))
        .collect()
}

fn default_values(command: &Command) -> BTreeMap<String, String> {
    command
        .get_arguments()
        .filter_map(|argument| {
            let values = argument.get_default_values();
            if values.is_empty() {
                return None;
            }
            assert_eq!(
                values.len(),
                1,
                "expected one default value for `{}`",
                argument.get_id()
            );
            Some((
                argument
                    .get_long()
                    .expect("defaulted option should have a long flag")
                    .to_owned(),
                values[0].to_string_lossy().into_owned(),
            ))
        })
        .collect()
}

fn value_enums(command: &Command) -> BTreeMap<String, String> {
    command
        .get_arguments()
        .filter(|argument| argument.get_action().takes_values())
        .filter_map(|argument| {
            let possible_values = argument.get_value_parser().possible_values()?;
            let mut names = possible_values
                .map(|value| value.get_name().to_owned())
                .collect::<Vec<_>>();
            names.sort_unstable();
            Some((
                argument
                    .get_long()
                    .expect("value-enum option should have a long flag")
                    .to_owned(),
                names.join("|"),
            ))
        })
        .collect()
}

fn visible_long_aliases(root: &Command) -> BTreeMap<String, String> {
    root.get_subcommands()
        .flat_map(|command| {
            command.get_arguments().filter_map(move |argument| {
                let mut aliases = argument.get_visible_aliases()?;
                aliases.sort_unstable();
                Some((
                    format!(
                        "{}.{}",
                        command.get_name(),
                        argument
                            .get_long()
                            .expect("aliased option should have a long flag")
                    ),
                    aliases.join("|"),
                ))
            })
        })
        .collect()
}

fn string_set(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn string_map(values: &[(&str, &str)]) -> BTreeMap<String, String> {
    values
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}
