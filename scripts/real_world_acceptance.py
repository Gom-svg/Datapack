#!/usr/bin/env python3
"""Bounded, local real-file acceptance through the installed public DataPack SDK.

No file contents or absolute input/work paths are written to the JSON report.
Outputs are retained unless --cleanup-output is explicitly requested.
"""

import argparse
import hashlib
import importlib
import importlib.metadata
import json
import math
import os
import platform
import shutil
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

SCHEMA_VERSION = 1
MIB = 1024 * 1024
GIB = 1024 * MIB
READ_SIZE = 4 * MIB
CHUNK_SIZE = 64 * MIB
RESERVE = 8 * GIB
REPORT_NAME = "real-world-acceptance.json"
CONFIG = {
    "chunk_size_bytes": CHUNK_SIZE,
    "threads": 4,
    "max_in_flight_chunks": 4,
    "backend": "chunked_raw_zstd",
    "adaptive_level": False,
    "max_memory_bytes": 256 * MIB,
}
DISCLAIMER = (
    "Timings are observational, include observer/verification overhead, and depend "
    "on storage, filesystem, caches, hardware and platform. Planner estimates from "
    "a sample are not guarantees or measurements of full-run behavior. The V2 "
    "admission limit is not a process-RSS ceiling."
)


class AcceptanceError(RuntimeError):
    """An acceptance requirement was not met."""


def require(condition, message):
    if not condition:
        raise AcceptanceError(message)


def fingerprint(path):
    info = path.stat()
    return (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns)


def sha256_stream(path):
    digest = hashlib.sha256()
    before = fingerprint(path)
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(READ_SIZE), b""):
            digest.update(block)
    require(fingerprint(path) == before, "File changed while calculating SHA-256")
    return digest.hexdigest()


def compare_streams(source, restored):
    """Independent full-file comparison and SHA-256 in one bounded read pass."""
    before = (fingerprint(source), fingerprint(restored))
    left_hash, right_hash = hashlib.sha256(), hashlib.sha256()
    equal, left_bytes, right_bytes = True, 0, 0
    with source.open("rb") as left, restored.open("rb") as right:
        while True:
            a, b = left.read(READ_SIZE), right.read(READ_SIZE)
            if not a and not b:
                break
            equal = equal and a == b
            left_hash.update(a)
            right_hash.update(b)
            left_bytes += len(a)
            right_bytes += len(b)
    require(
        before == (fingerprint(source), fingerprint(restored)),
        "A file changed during independent comparison",
    )
    return {
        "source_bytes": left_bytes,
        "restored_bytes": right_bytes,
        "size_match": left_bytes == right_bytes,
        "source_sha256": left_hash.hexdigest(),
        "restored_sha256": right_hash.hexdigest(),
        "sha256_match": left_hash.digest() == right_hash.digest(),
        "byte_for_byte_match": equal,
        "read_buffer_bytes_per_stream": READ_SIZE,
    }


def capacity_plan(size):
    # Reserve a near-incompressible archive, table/header overhead, and restoration.
    # New-file transactions rename their partial; no second whole output is copied.
    chunks = (size + CHUNK_SIZE - 1) // CHUNK_SIZE
    archive_budget = size + (size + 99) // 100 + chunks * 128 + MIB
    return {
        "archive_budget_bytes": archive_budget,
        "restored_budget_bytes": size,
        "reserve_bytes": RESERVE,
        "required_free_bytes": archive_budget + size + RESERVE,
        "source_copy_bytes": 0,
        "transaction_policy": "one new partial becomes the final file by commit; no overwrite",
        "space_policy": "source-size archive + 1% + table/header allowance, one restoration, 8 GiB reserve",
    }


def preflight(source, work_dir, free_bytes=None):
    source = source.resolve(strict=True)
    require(source.is_file(), "Input must be a regular file")
    size = source.stat().st_size
    require(size > 0, "Input must not be empty")
    require(
        not os.path.lexists(work_dir),
        "Work directory already exists; choose a new directory",
    )
    parent = work_dir.parent.resolve(strict=True)
    require(parent.is_dir(), "Work-directory parent must be an existing directory")
    repository = Path(__file__).resolve().parents[1]
    resolved_work = parent / work_dir.name
    if (repository / ".git").exists():
        require(
            repository not in resolved_work.parents,
            "Use a work directory outside the Git checkout",
        )
    plan = capacity_plan(size)
    free = shutil.disk_usage(parent).free if free_bytes is None else free_bytes
    require(
        free >= plan["required_free_bytes"],
        f"Insufficient work-volume capacity: need {plan['required_free_bytes']} bytes free "
        f"including reserve; found {free} bytes",
    )
    return (
        source,
        resolved_work,
        {**plan, "available_free_bytes": free, "input_bytes": size},
    )


class ProgressSummary:
    """Constant-space validation of synchronous P3 snapshots, not an event log."""

    def __init__(self, operation, ceiling, output=None):
        self.operation, self.ceiling, self.output = operation, ceiling, output
        self.count = self.terminals = 0
        self.last = None
        self.phases = {}
        self.violations = []
        self.committed_at_terminal = None
        self.active_phase = None
        self.previous_bytes = self.previous_items = 0

    def violation(self, message):
        if message not in self.violations and len(self.violations) < 16:
            self.violations.append(message)

    def __call__(self, event):
        self.count += 1
        if self.terminals:
            self.violation("event_after_terminal")
        if event.operation != self.operation:
            self.violation("wrong_operation")
        if event.completed_bytes < 0 or event.completed_bytes > self.ceiling:
            self.violation("implausible_completed_bytes")
        if event.total_bytes is not None:
            if not 0 <= event.completed_bytes <= event.total_bytes <= self.ceiling:
                self.violation("implausible_byte_total")
        if event.completed_items < 0 or (
            event.total_items is not None and event.completed_items > event.total_items
        ):
            self.violation("implausible_item_total")
        if event.percentage is not None and not (
            math.isfinite(event.percentage) and 0 <= event.percentage <= 100
        ):
            self.violation("invalid_percentage")
        if event.total_bytes is None and event.percentage is not None:
            self.violation("percentage_without_total")
        if event.state == "started" or event.stage != self.active_phase:
            self.active_phase = event.stage
            self.previous_bytes = self.previous_items = 0
        if (
            event.completed_bytes < self.previous_bytes
            or event.completed_items < self.previous_items
        ):
            self.violation("phase_counter_regressed")
        self.previous_bytes, self.previous_items = (
            event.completed_bytes,
            event.completed_items,
        )
        if event.stage not in self.phases and len(self.phases) >= 16:
            self.violation("unexpected_phase_count")
        else:
            phase = self.phases.setdefault(
                event.stage,
                {
                    "events": 0,
                    "advanced_events": 0,
                    "max_completed_bytes": 0,
                    "max_completed_items": 0,
                    "last_total_bytes": None,
                },
            )
            phase["events"] += 1
            phase["advanced_events"] += event.state == "advanced"
            phase["max_completed_bytes"] = max(
                phase["max_completed_bytes"], event.completed_bytes
            )
            phase["max_completed_items"] = max(
                phase["max_completed_items"], event.completed_items
            )
            phase["last_total_bytes"] = event.total_bytes
        self.last = {
            "stage": event.stage,
            "state": event.state,
            "terminal": event.terminal,
            "completed_bytes": event.completed_bytes,
            "total_bytes": event.total_bytes,
        }
        if event.terminal:
            self.terminals += 1
            if event.stage != "finalizing" or event.state != "completed":
                self.violation("invalid_terminal_identity")
            if self.output is not None:
                self.committed_at_terminal = self.output.is_file()
                if not self.committed_at_terminal:
                    self.violation("terminal_before_committed_output")

    def report(self):
        return {
            "event_count": self.count,
            "terminal_count": self.terminals,
            "last_event": self.last,
            "phases": self.phases,
            "violations": self.violations,
            "committed_output_at_terminal": self.committed_at_terminal,
        }

    def require_success(self):
        require(
            self.count > 0
            and self.terminals == 1
            and self.last["terminal"]
            and not self.violations,
            "Successful operation violated the progress contract",
        )


def cancellation_evidence(requested_bytes, error, progress, output, unexpected_files):
    return {
        "requested": requested_bytes is not None,
        "requested_after_bytes": requested_bytes,
        "exception_code": getattr(error, "code", None),
        "exception_category": getattr(error, "category", None),
        "cooperatively_cancelled": getattr(error, "code", None) == "cancelled",
        "false_terminal_success": progress.terminals != 0,
        "final_destination_absent": not os.path.lexists(output),
        "unexpected_or_partial_files": sorted(unexpected_files),
        "progress": progress.report(),
        "passed": (
            requested_bytes is not None
            and requested_bytes > 0
            and getattr(error, "code", None) == "cancelled"
            and getattr(error, "category", None) == "cancellation"
            and progress.terminals == 0
            and not progress.violations
            and not os.path.lexists(output)
            and not unexpected_files
        ),
    }


def write_report(work_dir, report):
    temporary = work_dir / (REPORT_NAME + ".tmp")
    with temporary.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(report, stream, indent=2, sort_keys=True, allow_nan=False)
        stream.write("\n")
    os.replace(temporary, work_dir / REPORT_NAME)


def error_record(error, source, work_dir):
    context = str(error)
    for path, label in ((source, "<input>"), (work_dir, "<work-dir>")):
        # OSError formats filenames with repr(), including doubled Windows
        # backslashes. Redact those spellings as well as ordinary SDK Display text.
        for spelling in (repr(str(path))[1:-1], str(path), path.as_posix()):
            context = context.replace(spelling, label)
    return {
        "type": type(error).__name__,
        "code": getattr(error, "code", "acceptance_failed"),
        "category": getattr(error, "category", "acceptance"),
        "context": context[:4096],
    }


def load_sdk():
    sdk = importlib.import_module("datapack")
    version = importlib.metadata.version("datapack-engine")
    require(sdk.__version__ == version, "Distribution/import versions do not agree")
    return sdk, version


def execute(args):
    # No workspace mutation occurs until identity, capacity, and streaming hash pass.
    source, work_dir, space = preflight(args.input, args.work_dir)
    sdk, version = load_sdk()
    identity = fingerprint(source)
    print(
        f"Preflight: {space['input_bytes']} input bytes; {space['available_free_bytes']} free; "
        f"{space['required_free_bytes']} required including reserve.",
        flush=True,
    )
    print("Hashing source with a bounded read buffer…", flush=True)
    started = time.monotonic()
    initial_hash = sha256_stream(source)
    if args.expected_sha256:
        require(
            initial_hash == args.expected_sha256,
            "Source SHA-256 differs from expected value",
        )
    require(fingerprint(source) == identity, "Source changed during preflight")
    require(
        shutil.disk_usage(work_dir.parent).free >= space["required_free_bytes"],
        "Available space dropped below the safety budget during source hashing",
    )
    report = {
        "schema_version": SCHEMA_VERSION,
        "status": "running",
        "profile": args.profile,
        "started_utc": datetime.now(timezone.utc).isoformat(),
        "environment": {
            "python": platform.python_version(),
            "platform": platform.platform(),
            "sdk_distribution": "datapack-engine",
            "datapack_version": version,
            "harness_sha256": sha256_stream(Path(__file__)),
        },
        "input": {
            "basename": source.name,
            "bytes": identity[2],
            "sha256": initial_hash,
        },
        "preflight": {**space, "source_hash_seconds": time.monotonic() - started},
        "configuration": CONFIG.copy(),
        "timing_disclaimer": DISCLAIMER,
        "cancel_retry_requested": args.cancel_retry,
        "outputs_retained": True,
        "stage": "preflight",
        "operations": {},
    }
    work_dir.mkdir(mode=0o700)
    archive, restored = work_dir / "archive.dpack", work_dir / "restored.csv"
    write_report(work_dir, report)
    size = identity[2]
    ceiling = space["archive_budget_bytes"]
    chunks = (size + CHUNK_SIZE - 1) // CHUNK_SIZE

    def source_unchanged():
        require(fingerprint(source) == identity, "Source identity/size/mtime changed")

    def operation(name, callback, output=None, observer=None):
        source_unchanged()
        report["stage"] = name
        write_report(work_dir, report)
        print(f"{name.capitalize()}…", flush=True)
        progress = observer or ProgressSummary(name, ceiling, output)
        began = time.monotonic()
        try:
            result = callback(progress)
        finally:
            report["operations"][name] = {
                "elapsed_seconds": time.monotonic() - began,
                "progress": progress.report(),
            }
        progress.require_success()
        source_unchanged()
        return result

    try:
        if args.profile == "full":
            analysis = operation("analyze", lambda p: sdk.analyze(source, progress=p))
            report["analysis"] = {
                "format": analysis["dataset"]["parser"]["format"],
                "delimiter": analysis["dataset"]["parser"]["delimiter"],
                "sampling": analysis["sampling"],
                "planner": analysis["planner"],
                "diagnostics": analysis["diagnostics"],
                "interpretation": "Sample-derived advice only; full V1 Structured is not executed.",
            }
            options = sdk.V2CompressionOptions(**CONFIG)
            require(
                shutil.disk_usage(work_dir).free >= space["required_free_bytes"],
                "Available space dropped below the full-run safety budget",
            )
            if args.cancel_retry:
                report["stage"] = "cancellation"
                write_report(work_dir, report)
                print(
                    "Cancellation probe: requesting a safe stop after at least 5% of input…",
                    flush=True,
                )
                token = sdk.CancellationToken()
                progress = ProgressSummary("compress", ceiling, archive)
                threshold = min(size, max(CHUNK_SIZE, (size + 19) // 20))
                requested, caught = None, None
                before_files = {p.name for p in work_dir.iterdir()}

                def cancel_after_progress(event):
                    nonlocal requested
                    progress(event)
                    if (
                        requested is None
                        and event.stage == "compressing"
                        and event.state == "advanced"
                        and event.completed_bytes >= threshold
                    ):
                        requested = event.completed_bytes
                        token.cancel()

                began = time.monotonic()
                try:
                    sdk.compress(
                        source,
                        archive,
                        options=options,
                        overwrite=False,
                        keep_partial=False,
                        progress=cancel_after_progress,
                        cancellation=token,
                    )
                except Exception as error:
                    caught = error
                unexpected = {p.name for p in work_dir.iterdir()} - before_files
                evidence = cancellation_evidence(
                    requested, caught, progress, archive, unexpected
                )
                evidence.update(
                    elapsed_seconds=time.monotonic() - began, trigger_bytes=threshold
                )
                report["cancellation"] = evidence
                if caught is not None and not isinstance(caught, sdk.CancelledError):
                    raise caught
                require(
                    evidence["passed"],
                    "Cancellation did not satisfy the public transaction contract",
                )
                source_unchanged()
                write_report(work_dir, report)
            require(not os.path.lexists(archive), "Archive destination already exists")
            require(
                shutil.disk_usage(work_dir).free >= space["required_free_bytes"],
                "Available space dropped below the full-run safety budget",
            )
            token = (
                sdk.CancellationToken()
            )  # A fresh token for the retry/full operation.
            compression = operation(
                "compress",
                lambda p: sdk.compress(
                    source,
                    archive,
                    options=options,
                    overwrite=False,
                    keep_partial=False,
                    progress=p,
                    cancellation=token,
                ),
                archive,
            )
            report["compression"] = compression
            require(
                compression["archive_version"] == 2
                and compression["selected_mode"] == "chunked_raw_zstd"
                and compression["backend"] == "chunked_raw_zstd",
                "Unexpected compression path",
            )
            require(
                compression["input_size_bytes"] == size
                and compression["archive_size_bytes"] == archive.stat().st_size,
                "Compression byte counts disagree with files",
            )
            archive_size = archive.stat().st_size
            report["compression"].update(
                ratio=size / archive_size,
                reduction_percent=(1 - archive_size / size) * 100,
                chunk_count_from_configuration=chunks,
            )
            if args.cancel_retry:
                report["cancellation"]["retry_succeeded"] = True
            validation = operation(
                "validate",
                lambda p: sdk.validate(
                    archive,
                    against=source,
                    max_output_bytes=size,
                    max_chunks=chunks,
                    max_memory_bytes=512 * MIB,
                    progress=p,
                ),
            )
            report["validation"] = validation
            require(
                validation["valid"] and validation["against"]["status"] == "matched",
                "Archive/source validation failed",
            )
            require(
                validation["archive"]["version"] == 2
                and validation["archive"]["chunk_count"] == chunks
                and all(value == "passed" for value in validation["checks"].values()),
                "A V2 validation check or chunk count did not pass",
            )
            report["compression"]["chunk_count"] = validation["archive"]["chunk_count"]
            free = shutil.disk_usage(work_dir).free
            report["space_before_restore"] = {
                "available_free_bytes": free,
                "required_free_bytes": size + RESERVE,
            }
            require(
                free >= size + RESERVE,
                "Insufficient space for restoration plus reserve",
            )
            require(
                not os.path.lexists(restored), "Restored destination already exists"
            )
            decompression = operation(
                "decompress",
                lambda p: sdk.decompress(
                    archive,
                    restored,
                    verify=True,
                    max_output_bytes=size,
                    max_chunks=chunks,
                    max_memory_bytes=512 * MIB,
                    overwrite=False,
                    keep_partial=False,
                    progress=p,
                ),
                restored,
            )
            report["decompression"] = decompression
            require(
                decompression["verified"] is True
                and decompression["restored_size_bytes"] == size,
                "Restoration was not fully verified or has the wrong length",
            )
            report["stage"] = "independent_verification"
            write_report(work_dir, report)
            print(
                "Independent streaming SHA-256 and byte-for-byte comparison…",
                flush=True,
            )
            began = time.monotonic()
            verification = compare_streams(source, restored)
            verification["elapsed_seconds"] = time.monotonic() - began
            verification["initial_source_hash_match"] = (
                verification["source_sha256"] == initial_hash
            )
            report["independent_verification"] = verification
            require(
                all(
                    verification[key]
                    for key in (
                        "size_match",
                        "sha256_match",
                        "byte_for_byte_match",
                        "initial_source_hash_match",
                    )
                ),
                "Independent exactness verification failed",
            )
            source_unchanged()
        report["status"] = "passed"
        report["acceptance_completed"] = args.profile == "full"
        report["stage"] = "complete"
        report["artifacts"] = {
            p.name: p.stat().st_size for p in (archive, restored) if p.exists()
        }
        report["space_after_run"] = {
            "available_free_bytes": shutil.disk_usage(work_dir).free,
            "artifact_bytes": sum(report["artifacts"].values()),
        }
        if not args.keep_output:
            # Only explicitly requested cleanup, only known successful-run outputs.
            for path in (archive, restored):
                if path.exists():
                    path.unlink()
            report["outputs_retained"] = False
    except Exception as error:
        report["status"] = "failed"
        report["acceptance_completed"] = False
        report["error"] = error_record(error, source, work_dir)
    finally:
        report["finished_utc"] = datetime.now(timezone.utc).isoformat()
        write_report(work_dir, report)
    if report["status"] != "passed":
        print(
            f"FAIL at {report['stage']}: {report['error']['code']}. Evidence retained.",
            flush=True,
        )
        return report, 1
    print(
        f"PASS — {args.profile}; DataPack {version}; {size} source bytes.", flush=True
    )
    if args.profile == "full":
        print(
            f"Archive: {archive.stat().st_size if args.keep_output else archive_size} bytes; "
            f"{report['compression']['ratio']:.4f}x; "
            f"{report['compression']['reduction_percent']:.2f}% reduction; {chunks} chunks.",
            flush=True,
        )
        print(
            "SIZE MATCH: PASS | SHA-256 MATCH: PASS | BYTE-FOR-BYTE MATCH: PASS",
            flush=True,
        )
    print(
        f"Report: {REPORT_NAME}. Outputs {'retained' if args.keep_output else 'removed by request'}. "
        "Timings are observational.",
        flush=True,
    )
    return report, 0


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument(
        "--work-dir",
        type=Path,
        required=True,
        help="New directory outside the checkout, with an existing parent",
    )
    parser.add_argument("--profile", choices=("full", "preflight"), default="full")
    parser.add_argument("--cancel-retry", action="store_true")
    group = parser.add_mutually_exclusive_group()
    group.add_argument(
        "--keep-output",
        dest="keep_output",
        action="store_true",
        default=True,
        help="Retain artifacts (the default)",
    )
    group.add_argument(
        "--cleanup-output",
        dest="keep_output",
        action="store_false",
        help="Explicitly remove successful archive/restoration; retain report",
    )
    parser.add_argument(
        "--expected-sha256", help="Optional independently known source SHA-256"
    )
    args = parser.parse_args(argv)
    if args.cancel_retry and args.profile != "full":
        parser.error("--cancel-retry requires --profile full")
    if args.expected_sha256:
        value = args.expected_sha256.lower()
        if len(value) != 64 or any(c not in "0123456789abcdef" for c in value):
            parser.error("--expected-sha256 must contain 64 hexadecimal characters")
        args.expected_sha256 = value
    return args


def main():
    args = parse_args()
    try:
        _, code = execute(args)
    except (
        AcceptanceError,
        OSError,
        ImportError,
        importlib.metadata.PackageNotFoundError,
    ) as error:
        print(f"Preflight refused: {type(error).__name__}: {error}", file=sys.stderr)
        return 2
    return code


if __name__ == "__main__":
    raise SystemExit(main())
