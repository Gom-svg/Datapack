#!/usr/bin/env python3
"""Run DataPack Technical Beta II through an installed Python wheel.

The harness creates deterministic synthetic data in an isolated temporary
directory, records privacy-safe metadata and hashes, and removes all generated
sources and archives on exit. Correctness assertions are gates. Wall-clock
measurements are observational only and never participate in pass/fail policy.
"""

import argparse
import filecmp
import hashlib
import importlib
import json
import os
import platform
import subprocess
import sys
import tempfile
import time
from importlib.metadata import distribution
from pathlib import Path

import datapack


HARNESS_VERSION = 1
MIB = 1024 * 1024
READ_SIZE = 128 * 1024
PROFILE_ROWS = {
    "ci": {
        "repetitive": 512,
        "realistic_structured": 768,
        "high_cardinality": 768,
        "random_incompressible_like": 1024,
        "etl_database_export": 2048,
    },
    "beta": {
        "repetitive": 80_000,
        "realistic_structured": 60_000,
        "high_cardinality": 40_000,
        "random_incompressible_like": 50_000,
        "etl_database_export": 120_000,
    },
}
SEEDS = {
    "repetitive": 0xDADA_6001,
    "realistic_structured": 0xDADA_6002,
    "high_cardinality": 0xDADA_6003,
    "random_incompressible_like": 0xDADA_6004,
    "etl_database_export": 0xDADA_6005,
}
EXPECTED_CI_INPUTS = {
    "repetitive": (
        32_940,
        "ca7c293cc0cf022f55291adb0e429ee17ebf650e1e8693e1c1cd5b178434345a",
        "csv_columnar_dictionary",
    ),
    "realistic_structured": (
        82_622,
        "8dafd26144a13df7a28c284976dfd6b99b9644451068325e0729b585821ea91c",
        "csv_columnar_dictionary",
    ),
    "high_cardinality": (
        136_558,
        "9e7a18da61145cd068f8c8257be4745746670205f8c6662a8547fc19e0945063",
        "raw_zstd",
    ),
    "random_incompressible_like": (
        299_036,
        "07e4cd3f06c3b2afe6b07c1329f6959cf270b999f79fae03851919a9047d1cc4",
        "raw_zstd",
    ),
    "etl_database_export": (
        278_887,
        "b479305411154a35d76e92ecb5234d6371bfb593e9f8f07e2d6fedf826e7bf8a",
        "csv_columnar_dictionary",
    ),
}


class HarnessFailure(RuntimeError):
    """A Technical Beta II correctness assertion failed."""


class SplitMix64:
    """Small, specified deterministic generator independent of Python random."""

    def __init__(self, seed):
        self.state = seed & 0xFFFFFFFFFFFFFFFF

    def next(self):
        self.state = (self.state + 0x9E3779B97F4A7C15) & 0xFFFFFFFFFFFFFFFF
        value = self.state
        value = ((value ^ (value >> 30)) * 0xBF58476D1CE4E5B9) & 0xFFFFFFFFFFFFFFFF
        value = ((value ^ (value >> 27)) * 0x94D049BB133111EB) & 0xFFFFFFFFFFFFFFFF
        return value ^ (value >> 31)

    def bounded(self, upper):
        return 0 if upper == 0 else self.next() % upper

    def token(self, alphabet, length):
        return "".join(alphabet[self.bounded(len(alphabet))] for _ in range(length))


def require(condition, message):
    if not condition:
        raise HarnessFailure(message)


def sha256_path(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(READ_SIZE), b""):
            digest.update(block)
    return digest.hexdigest()


def byte_equal(left, right):
    with left.open("rb") as left_file, right.open("rb") as right_file:
        while True:
            left_block = left_file.read(READ_SIZE)
            right_block = right_file.read(READ_SIZE)
            if left_block != right_block:
                return False
            if not left_block:
                return True


def timed(callable_value):
    started = time.perf_counter()
    result = callable_value()
    return result, round((time.perf_counter() - started) * 1000.0, 3)


def encode_field(value, delimiter):
    if any(character in value for character in (delimiter, '"', "\r", "\n")):
        return '"' + value.replace('"', '""') + '"'
    return value


def encode_rows(rows, delimiter=",", newline="\r\n", final_newline=True):
    rendered = []
    for row in rows:
        rendered.append(
            delimiter.join(encode_field(str(value), delimiter) for value in row)
        )
    text = newline.join(rendered)
    if final_newline:
        text += newline
    return text.encode("utf-8")


def repetitive_rows(count, rng):
    del rng
    rows = [
        ["record_id", "region", "status", "category", "amount", "code", "note", "empty"]
    ]
    regions = ("NA", "EU", "APAC", "LATAM")
    statuses = ("active", "pending", "closed")
    categories = ("standard", "priority")
    for index in range(count):
        rows.append(
            [
                str(index % 64),
                regions[index % len(regions)],
                statuses[index % len(statuses)],
                categories[index % len(categories)],
                "%0.2f" % ((index % 20) * 2.5),
                "%04d" % (index % 100),
                "stable repeated description",
                "",
            ]
        )
    return rows, ",", "\r\n"


def realistic_rows(count, rng):
    rows = [
        [
            "order_id",
            "timestamp",
            "customer_id",
            "region",
            "category",
            "amount",
            "status",
            "description",
            "optional",
        ]
    ]
    regions = ("north", "south", "east", "west")
    categories = ("hardware", "software", "service", "support")
    statuses = ("new", "approved", "fulfilled", "held")
    descriptions = (
        "quarterly renewal",
        "priority, account",
        'customer said "ship now"',
        "standard fulfillment",
        "revisión internacional",
    )
    for index in range(count):
        day = 1 + index % 28
        second = index % 60
        rows.append(
            [
                "ORD-%010d" % index,
                "2026-%02d-%02dT%02d:%02d:%02dZ"
                % (1 + index % 12, day, index % 24, index % 60, second),
                "CUST-%05d" % rng.bounded(400),
                regions[rng.bounded(len(regions))],
                categories[rng.bounded(len(categories))],
                "%d.%02d" % (100 + rng.bounded(90_000), rng.bounded(100)),
                statuses[rng.bounded(len(statuses))],
                descriptions[rng.bounded(len(descriptions))],
                "" if index % 11 == 0 else "present",
            ]
        )
    return rows, ",", "\r\n"


def high_cardinality_rows(count, rng):
    rows = [
        [
            "event_id",
            "trace_id",
            "sequence",
            "measurement",
            "scientific",
            "hex_payload",
            "unique_text",
        ]
    ]
    alphabet = "0123456789abcdef"
    for index in range(count):
        rows.append(
            [
                "EVT-%016x" % index,
                rng.token(alphabet, 32),
                "%012d" % index,
                "%d.%09d" % (index, rng.bounded(1_000_000_000)),
                "%.9e" % ((index + 1) * 1.23456789),
                rng.token(alphabet, 48),
                "unique-%d-%s" % (index, rng.token(alphabet, 20)),
            ]
        )
    return rows, ",", "\n"


def random_rows(count, rng):
    rows = [["blob_a", "blob_b", "blob_c", "blob_d"]]
    alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
    for _ in range(count):
        rows.append(
            [
                rng.token(alphabet, 72),
                rng.token(alphabet, 72),
                rng.token(alphabet, 72),
                rng.token(alphabet, 72),
            ]
        )
    return rows, ",", "\n"


def etl_rows(count, rng):
    rows = [
        [
            "export_id",
            "batch_date",
            "tenant",
            "account",
            "region",
            "currency",
            "gross",
            "tax",
            "net",
            "state",
            "source",
            "description",
            "nullable",
            "trailing_empty",
        ]
    ]
    tenants = ("tenant-a", "tenant-b", "tenant-c", "tenant-d")
    regions = ("us-east", "us-west", "eu-central", "ap-south", "ca-central")
    states = ("posted", "pending", "reversed", "settled")
    descriptions = (
        "nightly database export",
        "warehouse load",
        "backfill | reconciled",
        'operator "approved"',
        "cierre de período",
    )
    for index in range(count):
        gross_cents = 10_000 + rng.bounded(9_000_000)
        tax_cents = gross_cents * (index % 9) // 100
        rows.append(
            [
                "%014d" % index,
                "2026-%02d-%02d" % (1 + index % 12, 1 + index % 28),
                tenants[rng.bounded(len(tenants))],
                "ACCT-%06d" % rng.bounded(20_000),
                regions[rng.bounded(len(regions))],
                ("USD", "EUR", "CRC")[rng.bounded(3)],
                "%d.%02d" % divmod(gross_cents, 100),
                "%d.%02d" % divmod(tax_cents, 100),
                "%d.%02d" % divmod(gross_cents + tax_cents, 100),
                states[rng.bounded(len(states))],
                "db-primary-%02d" % (index % 8),
                descriptions[rng.bounded(len(descriptions))],
                "" if index % 7 == 0 else "value",
                "",
            ]
        )
    return rows, "|", "\r\n"


GENERATORS = {
    "repetitive": repetitive_rows,
    "realistic_structured": realistic_rows,
    "high_cardinality": high_cardinality_rows,
    "random_incompressible_like": random_rows,
    "etl_database_export": etl_rows,
}


def generate_workload(name, rows):
    seed = SEEDS[name]
    records, delimiter, newline = GENERATORS[name](rows, SplitMix64(seed))
    return encode_rows(records, delimiter, newline, True), delimiter, newline, seed


def analysis_fact(report):
    return {
        "parser_format": report["dataset"]["parser"]["format"],
        "delimiter": report["dataset"]["parser"]["delimiter"],
        "scope": report["sampling"]["scope"],
        "completeness": report["sampling"]["completeness"],
        "limited": report["sampling"]["limited"],
        "limit_reached": report["sampling"]["limit_reached"],
        "bytes_read": report["sampling"]["bytes_read"],
        "bytes_analyzed": report["sampling"]["bytes_analyzed"],
        "records_analyzed": report["sampling"]["records_analyzed"],
        "final_newline": report["sampling"]["final_newline"],
        "selection_scope": report["planner"]["selection_scope"],
        "selected_archive_mode": report["planner"]["selected_archive_mode"],
        "reason_code": report["planner"]["reason"]["code"],
        "reason_message": report["planner"]["reason"]["message"],
        "estimated_savings_percent": report["planner"]["estimated_savings_percent"],
        "estimated_dictionary_memory_mib": report["planner"][
            "estimated_dictionary_memory_mib"
        ],
        "diagnostic_codes": [item["code"] for item in report["diagnostics"]],
    }


def exact_roundtrip(
    source,
    archive,
    restored,
    options=None,
    sample_mb=1,
    allow_analysis_rejection=False,
):
    source_size = source.stat().st_size
    source_sha256 = sha256_path(source)
    analysis_error = None
    analysis_started = time.perf_counter()
    try:
        analysis = datapack.analyze(source, sample_mb=sample_mb)
    except datapack.DataPackError as error:
        if not allow_analysis_rejection:
            raise
        analysis = None
        analysis_error = {
            "type": type(error).__name__,
            "category": error.category,
            "code": error.code,
        }
    analysis_ms = round((time.perf_counter() - analysis_started) * 1000.0, 3)
    progress = []

    def compress_call():
        return datapack.compress(
            source, archive, options=options, progress=progress.append
        )

    compression, compression_ms = timed(compress_call)
    require(
        progress and progress[-1].terminal,
        "successful compression omitted terminal progress",
    )
    validation, validation_ms = timed(
        lambda: datapack.validate(archive, against=source)
    )
    require(validation["valid"], "successful archive did not validate")
    require(
        validation["against"]["status"] == "matched", "archive did not match its source"
    )
    decompression_progress = []
    decompression, decompression_ms = timed(
        lambda: datapack.decompress(
            archive, restored, progress=decompression_progress.append
        )
    )
    require(
        decompression_progress and decompression_progress[-1].terminal,
        "successful decompression omitted terminal progress",
    )
    restored_size = restored.stat().st_size
    restored_sha256 = sha256_path(restored)
    independent_compare = byte_equal(source, restored)
    require(source_size == restored_size, "restored byte count differs from source")
    require(source_sha256 == restored_sha256, "restored SHA-256 differs from source")
    require(
        filecmp.cmp(source, restored, shallow=False), "filecmp reported different bytes"
    )
    require(
        independent_compare, "independent streaming comparison reported different bytes"
    )
    archive_size = archive.stat().st_size
    return {
        "input_bytes": source_size,
        "input_sha256": source_sha256,
        "analysis": analysis_fact(analysis) if analysis is not None else None,
        "analysis_error": analysis_error,
        "archive_version": compression["archive_version"],
        "selected_mode": compression["selected_mode"],
        "backend": compression["backend"],
        "archive_bytes": archive_size,
        "archive_sha256": sha256_path(archive),
        "compression_ratio": round(source_size / archive_size, 6)
        if archive_size
        else None,
        "reduction_percent": (
            round((1.0 - archive_size / source_size) * 100.0, 6)
            if source_size
            else None
        ),
        "validation_valid": validation["valid"],
        "validation_against": validation["against"]["status"],
        "validation_diagnostic_codes": [
            item["code"] for item in validation["diagnostics"]
        ],
        "validation_checks": validation["checks"],
        "chunk_count": validation["archive"]["chunk_count"],
        "restored_bytes": restored_size,
        "restored_sha256": restored_sha256,
        "byte_for_byte_equal": independent_compare,
        "progress_terminal_success": True,
        "timings": {
            "classification": "observational",
            "analysis_ms": analysis_ms,
            "compression_ms": compression_ms,
            "validation_ms": validation_ms,
            "decompression_ms": decompression_ms,
        },
        "decompression": {
            "archive_version": decompression["archive_version"],
            "selected_mode": decompression["selected_mode"],
            "backend": decompression["backend"],
            "verified": decompression["verified"],
        },
    }


def run_workloads(workspace, profile):
    results = []
    for name, row_count in PROFILE_ROWS[profile].items():
        payload, delimiter, newline, seed = generate_workload(name, row_count)
        extension = "psv" if delimiter == "|" else "csv"
        source = workspace / (name + "." + extension)
        archive = workspace / (name + ".dpack")
        restored = workspace / (name + ".restored")
        source.write_bytes(payload)
        result = exact_roundtrip(
            source,
            archive,
            restored,
            options=datapack.V1CompressionOptions(sample_mb=1),
        )
        if profile == "ci":
            expected_size, expected_sha256, expected_mode = EXPECTED_CI_INPUTS[name]
            require(
                result["input_bytes"] == expected_size, name + " generator size changed"
            )
            require(
                result["input_sha256"] == expected_sha256,
                name + " generator hash changed",
            )
            require(
                result["selected_mode"] == expected_mode,
                name + " planner recommendation changed",
            )
        result.update(
            {
                "name": name,
                "generator": "technical_beta_ii_splitmix64_v1",
                "seed": seed,
                "rows": row_count,
                "delimiter": delimiter,
                "line_ending": "crlf" if newline == "\r\n" else "lf",
                "final_newline": True,
                "path_policy": "planner_selected_v1",
            }
        )
        results.append(result)

    etl_source = workspace / "etl_database_export.psv"
    v2_archive = workspace / "etl_database_export-v2.dpack"
    v2_restored = workspace / "etl_database_export-v2.restored"
    v2_options = datapack.V2CompressionOptions(
        chunk_size_bytes=64 * 1024,
        threads=2,
        max_in_flight_chunks=2,
        max_memory_bytes=128 * 1024,
    )
    v2_result = exact_roundtrip(etl_source, v2_archive, v2_restored, options=v2_options)
    require(v2_result["archive_version"] == 2, "forced v2 path did not write v2")
    require(
        v2_result["selected_mode"] == "chunked_raw_zstd", "forced v2 path mode changed"
    )
    require(
        v2_result["validation_checks"]["chunk_table"] == "passed",
        "v2 chunk table did not validate",
    )
    v2_result.update(
        {
            "name": "etl_database_export_v2_streaming",
            "generator": "technical_beta_ii_splitmix64_v1",
            "seed": SEEDS["etl_database_export"],
            "rows": PROFILE_ROWS[profile]["etl_database_export"],
            "delimiter": "|",
            "line_ending": "crlf",
            "final_newline": True,
            "path_policy": "forced_v2_compatibility_certification",
            "chunk_size_bytes": 64 * 1024,
            "threads": 2,
            "max_in_flight_chunks": 2,
        }
    )
    results.append(v2_result)
    return results


def rich_rows():
    return [
        [
            "id",
            "leading_zero",
            "scientific",
            "whitespace",
            "empty",
            "quoted",
            "unicode",
            "long_text",
            "trailing",
        ],
        [
            "1",
            "00042",
            "1.2300e+09",
            "  keep spaces  ",
            "",
            'said "hello"',
            "東京 / café / Δ",
            "x" * 4096,
            "",
        ],
        [
            "2",
            "00000",
            "-9.5E-03",
            "\ttext\t",
            "",
            "delimiter value",
            "vacío",
            "line one\nline two",
            "",
        ],
    ]


def run_format_matrix(workspace):
    cases = [
        ("csv_lf_final", ",", "\n", True),
        ("csv_crlf_no_final", ",", "\r\n", False),
        ("tsv_lf_no_final", "\t", "\n", False),
        ("psv_crlf_final", "|", "\r\n", True),
        ("semicolon_lf_final", ";", "\n", True),
    ]
    expected_formats = {",": "csv", "\t": "tsv", "|": "psv", ";": "semicolon_delimited"}
    results = []
    for name, delimiter, newline, final_newline in cases:
        rows = rich_rows()
        rows[2][5] = "contains " + delimiter + " delimiter"
        if delimiter == ",":
            # The canonical alternate-delimiter analyzer supports logical
            # multiline records. The frozen comma planner remains physical-line
            # based and intentionally rejects this representation.
            rows[2][7] = "line one / line two"
        source = workspace / (name + ".txt")
        source.write_bytes(encode_rows(rows, delimiter, newline, final_newline))
        result = exact_roundtrip(
            source,
            workspace / (name + ".dpack"),
            workspace / (name + ".restored"),
        )
        require(
            result["analysis"]["parser_format"] == expected_formats[delimiter],
            name + " dialect was not detected",
        )
        require(
            result["analysis"]["final_newline"] is final_newline,
            name + " newline fact changed",
        )
        result.update(
            {
                "name": name,
                "delimiter": delimiter,
                "line_ending": "crlf" if newline == "\r\n" else "lf",
                "final_newline": final_newline,
                "fidelity_cases": [
                    "quoted_fields",
                    "escaped_quotes",
                    "leading_zeros",
                    "scientific_notation",
                    "whitespace",
                    "empty_values",
                    "trailing_empty_fields",
                    "delimiter_inside_quotes",
                    "unicode",
                    "long_text",
                ]
                + ([] if delimiter == "," else ["multiline_quoted_value"]),
            }
        )
        results.append(result)
    return results


def error_fact(callable_value, expected_base=datapack.DataPackError):
    try:
        callable_value()
    except expected_base as error:
        return {
            "type": type(error).__name__,
            "category": error.category,
            "code": error.code,
        }
    raise HarnessFailure("operation unexpectedly succeeded")


def fallback_roundtrip(workspace, name, payload):
    source = workspace / (name + ".data")
    source.write_bytes(payload)
    result = exact_roundtrip(
        source,
        workspace / (name + ".dpack"),
        workspace / (name + ".restored"),
        allow_analysis_rejection=True,
    )
    return result


def run_edge_cases(workspace, profile):
    results = []

    empty = workspace / "empty.data"
    empty.write_bytes(b"")
    empty_analysis = error_fact(lambda: datapack.analyze(empty, sample_mb=1))
    empty_roundtrip = fallback_roundtrip(workspace, "empty-roundtrip", b"")
    results.append(
        {
            "name": "empty_file",
            "classification": "expected_rejection_plus_safe_raw_roundtrip",
            "analysis_error": empty_analysis,
            "compression_mode": empty_roundtrip["selected_mode"],
            "exact": empty_roundtrip["byte_for_byte_equal"],
            "input_bytes": 0,
            "input_sha256": hashlib.sha256(b"").hexdigest(),
        }
    )

    header_only = encode_rows([["a", "b", "c"]], ",", "\n", True)
    header_result = fallback_roundtrip(workspace, "header-only", header_only)
    results.append(
        {
            "name": "header_only",
            "classification": "pass",
            "analysis_records": header_result["analysis"]["records_analyzed"],
            "selected_mode": header_result["selected_mode"],
            "exact": header_result["byte_for_byte_equal"],
        }
    )

    one_row = encode_rows([["a", "b"], ["1", "2"]], ",", "\r\n", False)
    one_row_result = fallback_roundtrip(workspace, "one-row", one_row)
    results.append(
        {
            "name": "one_row",
            "classification": "pass",
            "analysis_records": one_row_result["analysis"]["records_analyzed"],
            "exact": one_row_result["byte_for_byte_equal"],
        }
    )

    single_column = b"value\nalpha\nbeta\n"
    single_path = workspace / "single-column.txt"
    single_path.write_bytes(single_column)
    single_error = error_fact(lambda: datapack.analyze(single_path, sample_mb=1))
    single_result = fallback_roundtrip(
        workspace, "single-column-roundtrip", single_column
    )
    results.append(
        {
            "name": "single_column",
            "classification": "expected_analysis_rejection_plus_safe_raw_roundtrip",
            "analysis_error": single_error,
            "selected_mode": single_result["selected_mode"],
            "exact": single_result["byte_for_byte_equal"],
        }
    )

    headerless = b"1|Ada\n2|Grace\n3|Linus\n"
    headerless_path = workspace / "headerless.psv"
    headerless_path.write_bytes(headerless)
    headerless_analysis = datapack.analyze(headerless_path, sample_mb=1)
    headerless_result = fallback_roundtrip(
        workspace, "headerless-roundtrip", headerless
    )
    results.append(
        {
            "name": "headerless_input",
            "classification": "limitation",
            "behavior": "first_record_header_policy",
            "header_mode": headerless_analysis["dataset"]["parser"]["header_mode"],
            "records_analyzed": headerless_analysis["sampling"]["records_analyzed"],
            "exact": headerless_result["byte_for_byte_equal"],
        }
    )

    malformed_cases = {
        "ambiguous_delimiter": b"a,b|c\n1,2|3\n",
        "inconsistent_width": b"a|b\n1|2\n3|4|5\n",
        "unterminated_quote": b'a|b\n1|"open\n',
        "bare_carriage_return": b"a|b\r1|2\r",
        "comma_multiline_quoted": b'id,note\n1,"line one\nline two"\n',
    }
    for name, payload in malformed_cases.items():
        source = workspace / (name + ".data")
        source.write_bytes(payload)
        analysis_error = error_fact(
            lambda source=source: datapack.analyze(source, sample_mb=1)
        )
        raw_result = fallback_roundtrip(workspace, name + "-roundtrip", payload)
        results.append(
            {
                "name": name,
                "classification": "expected_analysis_rejection_plus_safe_raw_roundtrip",
                "analysis_error": analysis_error,
                "selected_mode": raw_result["selected_mode"],
                "exact": raw_result["byte_for_byte_equal"],
            }
        )

    all_empty = encode_rows(
        [
            ["id", "empty_a", "empty_b", "state"],
            ["1", "", "", "ok"],
            ["2", "", "", "ok"],
        ],
        ",",
        "\n",
        True,
    )
    all_empty_result = fallback_roundtrip(workspace, "all-empty-columns", all_empty)
    results.append(
        {
            "name": "all_empty_columns",
            "classification": "pass",
            "exact": all_empty_result["byte_for_byte_equal"],
            "empty_value_counts": [
                column["empty_values"]
                for column in datapack.analyze(
                    workspace / "all-empty-columns.data", sample_mb=1
                )["dataset"]["columns"]
            ],
        }
    )

    wide_columns = 256
    wide_rows = [
        ["c%d" % index for index in range(wide_columns)],
        [str(index) for index in range(wide_columns)],
    ]
    wide_result = fallback_roundtrip(
        workspace,
        "many-columns",
        encode_rows(wide_rows, ";", "\r\n", True),
    )
    results.append(
        {
            "name": "many_columns_reasonable",
            "classification": "pass",
            "columns": wide_columns,
            "exact": wide_result["byte_for_byte_equal"],
        }
    )

    if profile == "beta":
        too_wide_columns = 4097
        too_wide = encode_rows(
            [
                ["c" for _ in range(too_wide_columns)],
                ["v" for _ in range(too_wide_columns)],
            ],
            "|",
            "\n",
            True,
        )
        too_wide_path = workspace / "too-wide.psv"
        too_wide_path.write_bytes(too_wide)
        analysis = datapack.analyze(too_wide_path, sample_mb=1)
        require(
            analysis["sampling"]["limit_reached"] == "column_limit",
            "wide limit changed",
        )
        results.append(
            {
                "name": "column_limit",
                "classification": "expected_bounded_fallback",
                "columns": too_wide_columns,
                "analysis": analysis_fact(analysis),
            }
        )

        oversized_record = b"left|right\n1|" + (b"x" * (8 * MIB + 1)) + b"\n"
        oversized_path = workspace / "oversized-record.psv"
        oversized_path.write_bytes(oversized_record)
        oversized_analysis = datapack.analyze(oversized_path, sample_mb=16)
        require(
            oversized_analysis["sampling"]["limit_reached"] == "record_byte_limit",
            "record byte limit changed",
        )
        results.append(
            {
                "name": "record_byte_limit",
                "classification": "expected_bounded_fallback",
                "input_bytes": len(oversized_record),
                "input_sha256": hashlib.sha256(oversized_record).hexdigest(),
                "analysis": analysis_fact(oversized_analysis),
            }
        )

    return results


def partial_paths(workspace):
    return sorted(
        path for path in workspace.iterdir() if path.name.endswith(".partial")
    )


def run_operational_resilience(workspace):
    source = workspace / "operational-source.csv"
    payload, _, _, _ = generate_workload("random_incompressible_like", 12_000)
    source.write_bytes(payload)
    options = datapack.V2CompressionOptions(
        chunk_size_bytes=64 * 1024,
        threads=1,
        max_in_flight_chunks=2,
        max_memory_bytes=128 * 1024,
    )
    sentinel = b"EXISTING_DESTINATION_MUST_SURVIVE"

    early = workspace / "early-cancel.dpack"
    early_token = datapack.CancellationToken()
    early_token.cancel()
    early_error = error_fact(
        lambda: datapack.compress(
            source, early, options=options, cancellation=early_token
        ),
        datapack.CancelledError,
    )
    require(not early.exists(), "early cancellation published an archive")

    mid = workspace / "mid-cancel.dpack"
    mid.write_bytes(sentinel)
    mid_token = datapack.CancellationToken()
    mid_events = []

    def cancel_mid(event):
        mid_events.append(event)
        if (
            event.stage == "compressing"
            and event.state == "advanced"
            and event.completed_items >= 1
        ):
            mid_token.cancel()

    mid_error = error_fact(
        lambda: datapack.compress(
            source,
            mid,
            options=options,
            overwrite=True,
            progress=cancel_mid,
            cancellation=mid_token,
        ),
        datapack.CancelledError,
    )
    require(
        mid.read_bytes() == sentinel,
        "mid compression cancellation replaced destination",
    )
    require(
        not any(event.terminal for event in mid_events),
        "cancelled compression reported terminal success",
    )
    require(
        not partial_paths(workspace), "mid cancellation retained an unrequested partial"
    )

    late = workspace / "late-cancel.dpack"
    late.write_bytes(sentinel)
    late_token = datapack.CancellationToken()
    late_events = []

    def cancel_late(event):
        late_events.append(event)
        if (
            event.stage == "compressing"
            and event.state == "advanced"
            and event.total_items is not None
            and event.completed_items == event.total_items
        ):
            late_token.cancel()

    late_error = error_fact(
        lambda: datapack.compress(
            source,
            late,
            options=options,
            overwrite=True,
            progress=cancel_late,
            cancellation=late_token,
        ),
        datapack.CancelledError,
    )
    require(
        late.read_bytes() == sentinel,
        "late compression cancellation replaced destination",
    )
    require(
        not any(event.terminal for event in late_events),
        "late cancellation reported terminal success",
    )

    retained = workspace / "retained-cancel.dpack"
    retained_token = datapack.CancellationToken()

    def cancel_retained(event):
        if event.stage == "compressing" and event.state == "advanced":
            retained_token.cancel()

    retained_error = error_fact(
        lambda: datapack.compress(
            source,
            retained,
            options=options,
            keep_partial=True,
            progress=cancel_retained,
            cancellation=retained_token,
        ),
        datapack.CancelledError,
    )
    retained_partials = partial_paths(workspace)
    require(
        len(retained_partials) == 1,
        "keep_partial cancellation did not retain exactly one partial",
    )
    retained_partial_bytes = retained_partials[0].stat().st_size
    retained_partials[0].unlink()

    archive = workspace / "retry-success.dpack"
    retry_events = []
    retry, retry_ms = timed(
        lambda: datapack.compress(
            source, archive, options=options, progress=retry_events.append
        )
    )
    require(
        retry_events and retry_events[-1].terminal, "retry compression did not complete"
    )
    validation = datapack.validate(archive, against=source)
    require(validation["valid"], "retry archive did not validate")

    restored = workspace / "decompress-cancelled.out"
    restored.write_bytes(sentinel)
    decompress_token = datapack.CancellationToken()
    decompress_events = []

    def cancel_decompress(event):
        decompress_events.append(event)
        if (
            event.stage == "decompressing"
            and event.state == "advanced"
            and event.completed_items >= 1
        ):
            decompress_token.cancel()

    decompress_error = error_fact(
        lambda: datapack.decompress(
            archive,
            restored,
            overwrite=True,
            progress=cancel_decompress,
            cancellation=decompress_token,
        ),
        datapack.CancelledError,
    )
    require(
        restored.read_bytes() == sentinel,
        "decompression cancellation replaced destination",
    )
    require(
        not any(event.terminal for event in decompress_events),
        "cancelled decompression reported success",
    )
    require(
        not partial_paths(workspace),
        "decompression cancellation retained an unrequested partial",
    )

    retained_restored = workspace / "decompress-retained.out"
    retained_decompress_token = datapack.CancellationToken()

    def cancel_retained_decompress(event):
        if (
            event.stage == "decompressing"
            and event.state == "advanced"
            and event.completed_items >= 1
        ):
            retained_decompress_token.cancel()

    retained_decompress_error = error_fact(
        lambda: datapack.decompress(
            archive,
            retained_restored,
            keep_partial=True,
            progress=cancel_retained_decompress,
            cancellation=retained_decompress_token,
        ),
        datapack.CancelledError,
    )
    require(
        not retained_restored.exists(), "cancelled decompression published final output"
    )
    decompression_partials = partial_paths(workspace)
    require(
        len(decompression_partials) == 1,
        "decompression keep_partial did not retain exactly one partial",
    )
    retained_decompression_bytes = decompression_partials[0].stat().st_size
    decompression_partials[0].unlink()

    restored_retry = workspace / "retry-restored.csv"
    decompression = datapack.decompress(archive, restored_retry)
    require(
        byte_equal(source, restored_retry), "retry decompression was not byte exact"
    )

    overwrite_archive = workspace / "overwrite-success.dpack"
    overwrite_archive.write_bytes(sentinel)
    protected_error = error_fact(
        lambda: datapack.compress(source, overwrite_archive, options=options),
        datapack.DataPackFormatError,
    )
    require(
        overwrite_archive.read_bytes() == sentinel,
        "safe overwrite default replaced destination",
    )
    datapack.compress(source, overwrite_archive, options=options, overwrite=True)
    require(
        overwrite_archive.read_bytes() != sentinel, "explicit overwrite did not commit"
    )

    return {
        "early_compression_cancellation": {
            "classification": "pass",
            "error": early_error,
            "final_output_exists": early.exists(),
        },
        "mid_compression_cancellation": {
            "classification": "pass",
            "error": mid_error,
            "existing_destination_preserved": mid.read_bytes() == sentinel,
            "terminal_success_seen": any(event.terminal for event in mid_events),
        },
        "late_compression_cancellation": {
            "classification": "pass",
            "error": late_error,
            "existing_destination_preserved": late.read_bytes() == sentinel,
            "terminal_success_seen": any(event.terminal for event in late_events),
        },
        "keep_partial_cancellation": {
            "classification": "pass",
            "error": retained_error,
            "final_output_exists": retained.exists(),
            "retained_partial_bytes": retained_partial_bytes,
        },
        "retry_compression": {
            "classification": "pass",
            "archive_version": retry["archive_version"],
            "selected_mode": retry["selected_mode"],
            "archive_bytes": archive.stat().st_size,
            "validation_valid": validation["valid"],
            "timing_ms": retry_ms,
            "timing_classification": "observational",
        },
        "decompression_cancellation": {
            "classification": "pass",
            "error": decompress_error,
            "existing_destination_preserved": restored.read_bytes() == sentinel,
            "terminal_success_seen": any(event.terminal for event in decompress_events),
        },
        "decompression_keep_partial_cancellation": {
            "classification": "pass",
            "error": retained_decompress_error,
            "final_output_exists": retained_restored.exists(),
            "retained_partial_bytes": retained_decompression_bytes,
        },
        "retry_decompression": {
            "classification": "pass",
            "restored_bytes": decompression["restored_size_bytes"],
            "source_sha256": sha256_path(source),
            "restored_sha256": sha256_path(restored_retry),
            "byte_for_byte_equal": byte_equal(source, restored_retry),
        },
        "overwrite_policy": {
            "classification": "pass",
            "default_rejection": protected_error,
            "default_preserved_destination": True,
            "explicit_overwrite_committed": True,
        },
    }


def diagnostic_codes(report):
    return [diagnostic["code"] for diagnostic in report["diagnostics"]]


def run_resource_and_validation(workspace):
    source = workspace / "resource-source.csv"
    payload, _, _, _ = generate_workload("repetitive", 20_000)
    source.write_bytes(payload)
    options = datapack.V2CompressionOptions(
        chunk_size_bytes=64 * 1024,
        threads=1,
        max_in_flight_chunks=2,
        max_memory_bytes=128 * 1024,
    )
    archive = workspace / "resource-v2.dpack"
    datapack.compress(source, archive, options=options)
    valid = datapack.validate(archive, against=source)
    require(valid["valid"], "resource test archive did not validate")
    chunks = valid["archive"]["chunk_count"]
    require(chunks > 1, "resource test did not create multiple chunks")

    refused_archive = workspace / "memory-refused.dpack"
    refused_options = datapack.V2CompressionOptions(
        chunk_size_bytes=64 * 1024,
        threads=1,
        max_in_flight_chunks=2,
        max_memory_bytes=64 * 1024,
    )
    compression_memory_error = error_fact(
        lambda: datapack.compress(source, refused_archive, options=refused_options)
    )
    require(not refused_archive.exists(), "memory-refused compression published output")

    output_limit = datapack.validate(
        archive, max_output_bytes=source.stat().st_size - 1
    )
    chunk_limit = datapack.validate(archive, max_chunks=1)
    memory_limit = datapack.validate(archive, max_memory_bytes=1)
    require(not output_limit["valid"], "validation output limit was not enforced")
    require(not chunk_limit["valid"], "validation chunk limit was not enforced")
    require(not memory_limit["valid"], "validation memory limit was not enforced")

    limit_outputs = []
    for name, keyword in (
        ("output", {"max_output_bytes": source.stat().st_size - 1}),
        ("chunks", {"max_chunks": 1}),
        ("memory", {"max_memory_bytes": 1}),
    ):
        destination = workspace / ("decompress-limit-" + name)
        destination.write_bytes(b"PRESERVE")
        error = error_fact(
            lambda destination=destination, keyword=keyword: datapack.decompress(
                archive, destination, overwrite=True, **keyword
            )
        )
        require(
            destination.read_bytes() == b"PRESERVE",
            name + " limit replaced destination",
        )
        limit_outputs.append(
            {
                "limit": name,
                "error": error,
                "existing_destination_preserved": True,
            }
        )

    v1_archive = workspace / "resource-v1.dpack"
    datapack.compress(source, v1_archive)
    v1_memory = datapack.validate(v1_archive, max_memory_bytes=1)
    require(
        not v1_memory["valid"], "v1 structured validation memory limit was not enforced"
    )

    mismatch_source = workspace / "different-source.csv"
    mismatch_source.write_bytes(b"different source bytes\n")
    mismatch = datapack.validate(archive, against=mismatch_source)
    require(not mismatch["valid"], "source mismatch was reported valid")
    require(
        mismatch["against"]["status"] == "mismatched", "source mismatch status changed"
    )

    original_archive = archive.read_bytes()
    corruptions = {}
    mutations = {
        "truncated": original_archive[:-8],
        "tampered_payload": original_archive[:-1]
        + bytes([original_archive[-1] ^ 0x5A]),
        "trailing_garbage": original_archive + b"P6_PRIVATE_TRAILING_MARKER",
        "malformed_archive": b"not-a-dpack-archive",
    }
    for name, mutation in mutations.items():
        path = workspace / (name + ".dpack")
        path.write_bytes(mutation)
        report = datapack.validate(path)
        require(not report["valid"], name + " corruption was reported valid")
        codes = diagnostic_codes(report)
        require(codes, name + " corruption omitted a diagnostic code")
        corruptions[name] = {
            "classification": "expected_invalid_result",
            "valid": report["valid"],
            "diagnostic_codes": codes,
            "checks": report["checks"],
        }

    missing = workspace / "missing.dpack"
    missing_error = error_fact(lambda: datapack.validate(missing))

    sampled_source = workspace / "sample-limited.csv"
    sampled_payload, _, _, _ = generate_workload("random_incompressible_like", 12_000)
    sampled_source.write_bytes(sampled_payload)
    sampled = datapack.analyze(sampled_source, sample_mb=1)
    require(
        sampled["sampling"]["scope"] == "sampled", "sampled analysis claimed full scope"
    )
    require(
        sampled["sampling"]["completeness"] == "partial",
        "sampled analysis claimed complete",
    )

    return {
        "valid_v2": {
            "classification": "pass",
            "archive_bytes": archive.stat().st_size,
            "archive_sha256": sha256_path(archive),
            "chunk_count": chunks,
            "checks": valid["checks"],
        },
        "compression_memory_admission": {
            "classification": "expected_rejection",
            "error": compression_memory_error,
            "final_output_exists": refused_archive.exists(),
        },
        "validation_limits": {
            "output": diagnostic_codes(output_limit),
            "chunks": diagnostic_codes(chunk_limit),
            "memory": diagnostic_codes(memory_limit),
            "v1_structured_memory": diagnostic_codes(v1_memory),
        },
        "decompression_limits": limit_outputs,
        "source_mismatch": {
            "classification": "expected_invalid_result",
            "valid": mismatch["valid"],
            "against": mismatch["against"]["status"],
            "diagnostic_codes": diagnostic_codes(mismatch),
        },
        "corruption": corruptions,
        "operational_failure": {
            "classification": "expected_exception",
            "missing_archive": missing_error,
        },
        "sampled_analysis": analysis_fact(sampled),
    }


def run_compare_evidence(workspace):
    source = workspace / "compare-source.csv"
    payload, _, _, _ = generate_workload("realistic_structured", 512)
    source.write_bytes(payload)
    quick, quick_ms = timed(lambda: datapack.compare(source, mode="quick", runs=1))
    full, full_ms = timed(lambda: datapack.compare(source, mode="full", runs=1))
    for name, report in (("quick", quick), ("full", full)):
        require(
            report["datapack"]["validation"]["sha256_match"],
            name + " DataPack compare mismatch",
        )
        require(
            report["standalone_zstd"]["validation"]["sha256_match"],
            name + " zstd compare mismatch",
        )
    token = datapack.CancellationToken()
    token.cancel()
    cancelled = error_fact(
        lambda: datapack.compare(source, mode="full", runs=1, cancellation=token),
        datapack.CancelledError,
    )
    missing = error_fact(
        lambda: datapack.compare(workspace / "missing-compare.csv", runs=1)
    )
    return {
        "quick": {
            "classification": "pass",
            "scope": quick["scope"],
            "datapack_validation": quick["datapack"]["validation"],
            "zstd_validation": quick["standalone_zstd"]["validation"],
            "winners_are_factual_differences": quick["winners"],
            "limitations": quick["limitations"],
            "timing_ms": quick_ms,
            "timing_classification": "observational",
        },
        "full": {
            "classification": "pass",
            "scope": full["scope"],
            "datapack_validation": full["datapack"]["validation"],
            "zstd_validation": full["standalone_zstd"]["validation"],
            "winners_are_factual_differences": full["winners"],
            "limitations": full["limitations"],
            "timing_ms": full_ms,
            "timing_classification": "observational",
        },
        "cancellation": {"classification": "pass", "error": cancelled},
        "operational_failure": {
            "classification": "expected_exception",
            "error": missing,
        },
    }


def run_command(command, expected_codes=(0,)):
    completed = subprocess.run(command, check=False, capture_output=True)
    require(
        completed.returncode in expected_codes,
        "unexpected CLI exit status %d for %r; stderr=%r"
        % (
            completed.returncode,
            command,
            completed.stderr.decode("utf-8", errors="replace"),
        ),
    )
    return completed


def parse_json_stdout(completed, label):
    require(completed.stdout.endswith(b"\n"), label + " JSON did not end with newline")
    require(
        completed.stdout.count(b"\n") == 1, label + " compact JSON was not stdout-clean"
    )
    return json.loads(completed.stdout.decode("utf-8"))


def run_cli_evidence(workspace, cli_path):
    cli = str(cli_path.resolve())
    version = run_command([cli, "--version"])
    help_result = run_command([cli, "--help"])
    require(
        b"datapack" in version.stdout.lower(), "CLI version output omitted identity"
    )
    require(
        b"transactional" in help_result.stdout.lower(),
        "CLI help omitted transactional safety",
    )

    source = workspace / "cli-source.csv"
    payload, _, _, _ = generate_workload("realistic_structured", 1024)
    source.write_bytes(payload)
    archive = workspace / "cli-source.dpack"
    restored = workspace / "cli-restored.csv"

    analysis_process = run_command(
        [cli, "analyze", str(source), "--sample-mb", "1", "--json"]
    )
    analysis = parse_json_stdout(analysis_process, "analyze")
    compression = run_command(
        [cli, "compress", str(source), str(archive), "--sample-mb", "1"]
    )
    require(not compression.stdout, "compress wrote scripting noise to stdout")
    validation_process = run_command(
        [cli, "validate", str(archive), "--against", str(source), "--json"]
    )
    validation = parse_json_stdout(validation_process, "validate")
    require(validation["valid"], "CLI archive validation failed")
    require(
        not restored.exists(), "CLI destination was pre-created before decompression"
    )
    decompression = run_command([cli, "decompress", str(archive), str(restored)])
    require(not decompression.stdout, "decompress wrote scripting noise to stdout")
    require(byte_equal(source, restored), "CLI restored bytes differ")

    safe_overwrite = run_command(
        [cli, "decompress", str(archive), str(restored)], expected_codes=(1,)
    )
    require(not safe_overwrite.stdout, "overwrite failure polluted stdout")
    require(
        b"error[invalid_format]" in safe_overwrite.stderr,
        "stable overwrite error missing",
    )

    mismatch = workspace / "cli-mismatch.csv"
    mismatch.write_bytes(b"mismatch\n")
    negative_process = run_command(
        [cli, "validate", str(archive), "--against", str(mismatch), "--json"],
        expected_codes=(1,),
    )
    negative = parse_json_stdout(negative_process, "negative validate")
    require(not negative["valid"], "CLI mismatch validation returned valid")
    require(
        b"error[invalid_format]" in negative_process.stderr,
        "negative validation error missing",
    )

    missing_process = run_command(
        [
            cli,
            "compress",
            str(workspace / "missing.csv"),
            str(workspace / "missing.dpack"),
        ],
        expected_codes=(1,),
    )
    require(not missing_process.stdout, "CLI operational failure polluted stdout")
    require(
        b"error[" in missing_process.stderr,
        "CLI operational failure omitted stable rendering",
    )

    compare_process = run_command(
        [cli, "compare", str(source), "--mode", "quick", "--runs", "1", "--json"]
    )
    comparison = parse_json_stdout(compare_process, "compare")

    return {
        "classification": "pass",
        "version": version.stdout.decode("utf-8").strip(),
        "help_usable": True,
        "analyze_json_stdout_clean": analysis["report_type"] == "analysis",
        "compress_stdout_clean": not compression.stdout,
        "validate_json_stdout_clean": validation["report_type"] == "validation",
        "decompress_stdout_clean": not decompression.stdout,
        "compare_json_stdout_clean": comparison["report_type"] == "comparison",
        "safe_overwrite_exit_code": safe_overwrite.returncode,
        "validation_negative_exit_code": negative_process.returncode,
        "operational_failure_exit_code": missing_process.returncode,
        "stable_error_rendering": True,
        "source_bytes": source.stat().st_size,
        "restored_bytes": restored.stat().st_size,
        "source_sha256": sha256_path(source),
        "restored_sha256": sha256_path(restored),
        "byte_for_byte_equal": byte_equal(source, restored),
    }


def environment_facts(forbidden_source_root):
    installed = distribution("datapack-engine")
    native = importlib.import_module("datapack._native")
    package_path = Path(datapack.__file__).resolve()
    native_path = Path(native.__file__).resolve()
    facts = {
        "distribution": installed.metadata["Name"],
        "distribution_version": installed.version,
        "import": "datapack",
        "import_version": datapack.__version__,
        "package_file": str(package_path),
        "native_file": str(native_path),
        "python": platform.python_version(),
        "implementation": platform.python_implementation(),
        "platform": platform.platform(),
        "os_name": os.name,
        "executable": str(Path(sys.executable).resolve()),
        "repository_source_absent_from_cwd_and_sys_path": None,
        "rust_toolchain_required_at_runtime": False,
        "network_calls": False,
        "telemetry": False,
        "raw_rows_in_report": False,
    }
    if forbidden_source_root is not None:
        forbidden = forbidden_source_root.resolve()

        def within(path, parent):
            try:
                path.resolve().relative_to(parent)
            except ValueError:
                return False
            return True

        isolated = not within(Path.cwd(), forbidden)
        isolated = isolated and not within(package_path, forbidden)
        isolated = isolated and not within(native_path, forbidden)
        for entry in sys.path:
            if entry:
                isolated = isolated and not within(Path(entry), forbidden)
        facts["repository_source_absent_from_cwd_and_sys_path"] = isolated
        require(
            isolated, "repository source leaked into installed-wheel beta environment"
        )
    require(
        facts["distribution"] == "datapack-engine",
        "installed distribution identity changed",
    )
    require(
        facts["distribution_version"] == datapack.__version__,
        "installed version mismatch",
    )
    return facts


def run(profile, cli_path, forbidden_source_root):
    started = time.perf_counter()
    environment = environment_facts(forbidden_source_root)
    with tempfile.TemporaryDirectory(prefix="datapack-technical-beta-ii-") as temporary:
        workspace = Path(temporary).resolve()
        report = {
            "schema_version": 1,
            "report_type": "technical_beta_ii",
            "harness_version": HARNESS_VERSION,
            "result": "PASS",
            "evidence_classification": {
                "correctness": "certification_gate",
                "wall_clock_performance": "observational_only",
                "timing_failure_thresholds": False,
            },
            "profile": profile,
            "size_policy": {
                "ci": "small deterministic proxies suitable for routine hosted regression",
                "beta": "manual local technical-beta scale with streaming/chunking and hard-limit probes",
                "multi_gigabyte_required": False,
            },
            "environment": environment,
            "privacy_and_security": {
                "temporary_local_generation": True,
                "generated_artifacts_removed_on_exit": True,
                "network_calls": False,
                "telemetry": False,
                "automatic_upload": False,
                "raw_rows_recorded": False,
                "report_contains_only_metadata_hashes_and_diagnostics": True,
            },
            "workloads": run_workloads(workspace, profile),
            "format_matrix": run_format_matrix(workspace),
            "edge_cases": run_edge_cases(workspace, profile),
            "operational_resilience": run_operational_resilience(workspace),
            "resource_and_validation": run_resource_and_validation(workspace),
            "compare": run_compare_evidence(workspace),
            "cli": None,
        }
        if cli_path is not None:
            require(cli_path.is_file(), "CLI path is not a file: " + str(cli_path))
            report["cli"] = run_cli_evidence(workspace, cli_path)
        report["total_elapsed_ms"] = round((time.perf_counter() - started) * 1000.0, 3)
        report["total_elapsed_classification"] = "observational"
        return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", choices=("ci", "beta"), default="ci")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--cli", type=Path)
    parser.add_argument("--forbidden-source-root", type=Path)
    arguments = parser.parse_args()

    report = run(arguments.profile, arguments.cli, arguments.forbidden_source_root)
    rendered = json.dumps(report, sort_keys=True, indent=2) + "\n"
    if arguments.output is not None:
        arguments.output.parent.mkdir(parents=True, exist_ok=True)
        arguments.output.write_text(rendered, encoding="utf-8")
    sys.stdout.write(rendered)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (HarnessFailure, OSError, ValueError) as error:
        print("Technical Beta II: FAIL: " + str(error), file=sys.stderr)
        raise SystemExit(1) from error
