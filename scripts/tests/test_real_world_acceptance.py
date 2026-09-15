"""Tiny-fixture checks for the external SDK acceptance consumer; no real data."""

import contextlib
import hashlib
import io
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path, PureWindowsPath
from types import SimpleNamespace
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import real_world_acceptance as acceptance  # noqa: E402


def event(**overrides):
    values = dict(
        operation="compress",
        stage="compressing",
        state="advanced",
        completed_bytes=10,
        total_bytes=100,
        completed_items=1,
        total_items=10,
        percentage=10.0,
        terminal=False,
    )
    values.update(overrides)
    return SimpleNamespace(**values)


def terminal():
    return event(
        stage="finalizing",
        state="completed",
        completed_bytes=0,
        total_bytes=None,
        completed_items=0,
        total_items=None,
        percentage=None,
        terminal=True,
    )


class LocalFiles(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.source = self.root / "synthetic.csv"
        self.source.write_bytes(b"id,amount,kind\r\n1,100,A\r\n2,200,B\r\n")
        self.work = self.root / "evidence"

    def args(self, *extras):
        return acceptance.parse_args(
            ["--input", str(self.source), "--work-dir", str(self.work), *extras]
        )


class SafetyAndVerificationTests(LocalFiles):
    def test_argument_validation_and_safe_defaults(self):
        args = self.args()
        self.assertEqual(args.profile, "full")
        self.assertTrue(args.keep_output)
        self.assertFalse(args.cancel_retry)
        self.assertFalse(self.args("--cleanup-output").keep_output)
        for extra in (
            ("--profile", "unknown"),
            ("--profile", "preflight", "--cancel-retry"),
            ("--expected-sha256", "bad"),
            ("--cleanup-output", "--keep-output"),
        ):
            with self.subTest(extra=extra), contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit):
                    self.args(*extra)
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            acceptance.parse_args([])

    def test_capacity_uses_incompressible_archive_and_exact_free_boundary(self):
        size = self.source.stat().st_size
        plan = acceptance.capacity_plan(size)
        self.assertGreater(plan["archive_budget_bytes"], size)
        self.assertEqual(plan["restored_budget_bytes"], size)
        self.assertEqual(plan["source_copy_bytes"], 0)
        self.assertEqual(plan["reserve_bytes"], 8 * acceptance.GIB)
        required = plan["required_free_bytes"]
        acceptance.preflight(self.source, self.work, free_bytes=required)
        with self.assertRaisesRegex(acceptance.AcceptanceError, "Insufficient"):
            acceptance.preflight(self.source, self.work, free_bytes=required - 1)
        self.assertFalse(self.work.exists())

    def test_existing_workspace_sentinel_is_never_overwritten(self):
        self.work.mkdir()
        sentinel = self.work / "archive.dpack"
        sentinel.write_bytes(b"preserve existing evidence")
        with self.assertRaisesRegex(acceptance.AcceptanceError, "already exists"):
            acceptance.preflight(self.source, self.work, free_bytes=10**12)
        self.assertEqual(sentinel.read_bytes(), b"preserve existing evidence")

    def test_dangling_workspace_symlink_is_refused(self):
        try:
            self.work.symlink_to(self.root / "missing", target_is_directory=True)
        except OSError:
            self.skipTest("Host does not allow unprivileged symlinks")
        with self.assertRaisesRegex(acceptance.AcceptanceError, "already exists"):
            acceptance.preflight(self.source, self.work, free_bytes=10**12)
        self.assertTrue(os.path.lexists(self.work))

    def test_missing_empty_directory_and_checkout_inputs_are_refused(self):
        with self.assertRaises(FileNotFoundError):
            acceptance.preflight(self.root / "missing", self.work)
        with self.assertRaisesRegex(acceptance.AcceptanceError, "regular file"):
            acceptance.preflight(self.root, self.work)
        self.source.write_bytes(b"")
        with self.assertRaisesRegex(acceptance.AcceptanceError, "empty"):
            acceptance.preflight(self.source, self.work)
        self.source.write_bytes(b"nonempty")
        checkout = Path(acceptance.__file__).resolve().parents[1]
        if (checkout / ".git").exists():
            with self.assertRaisesRegex(acceptance.AcceptanceError, "outside"):
                acceptance.preflight(
                    self.source, checkout / "forbidden-real-data", free_bytes=10**12
                )

    def test_streaming_sha_and_independent_comparison_across_buffer_boundaries(self):
        data = self.source.read_bytes()
        restored = self.root / "restored.csv"
        restored.write_bytes(data)
        with mock.patch.object(acceptance, "READ_SIZE", 7):
            self.assertEqual(
                acceptance.sha256_stream(self.source), hashlib.sha256(data).hexdigest()
            )
            result = acceptance.compare_streams(self.source, restored)
        self.assertTrue(result["size_match"])
        self.assertTrue(result["sha256_match"])
        self.assertTrue(result["byte_for_byte_match"])
        self.assertEqual(result["source_bytes"], len(data))
        self.assertEqual(result["read_buffer_bytes_per_stream"], 7)

    def test_independent_comparison_detects_changed_byte_truncation_and_trailing_data(
        self,
    ):
        data = self.source.read_bytes()
        restored = self.root / "restored.csv"
        for changed in (b"X" + data[1:], data[:-1], data + b"extra"):
            with self.subTest(length=len(changed)):
                restored.write_bytes(changed)
                result = acceptance.compare_streams(self.source, restored)
                self.assertFalse(result["byte_for_byte_match"])
                self.assertFalse(result["sha256_match"])
                self.assertEqual(result["size_match"], len(data) == len(changed))

    def test_changed_source_identity_is_detected_while_hashing(self):
        with mock.patch.object(acceptance, "fingerprint", side_effect=[(1,), (2,)]):
            with self.assertRaisesRegex(acceptance.AcceptanceError, "changed"):
                acceptance.sha256_stream(self.source)

    def test_json_report_roundtrip_and_path_redaction(self):
        self.work.mkdir()
        error = OSError(f"Cannot read {self.source} or write {self.work}/archive.dpack")
        report = {
            "schema_version": 1,
            "status": "failed",
            "error": acceptance.error_record(error, self.source, self.work),
        }
        acceptance.write_report(self.work, report)
        acceptance.write_report(self.work, report)
        encoded = (self.work / acceptance.REPORT_NAME).read_text()
        self.assertEqual(json.loads(encoded), report)
        self.assertNotIn(str(self.root), encoded)
        self.assertEqual(
            list(self.work.iterdir()), [self.work / acceptance.REPORT_NAME]
        )

    def test_windows_oserror_escaped_filename_is_redacted(self):
        source = PureWindowsPath("C:/private/source.csv")
        work = PureWindowsPath("C:/private/work")
        for path in (source, work / "archive.dpack"):
            error = FileNotFoundError(2, "File not found", str(path))
            result = acceptance.error_record(error, source, work)
            self.assertNotIn("private", result["context"])
            self.assertIn(
                "<input>" if path == source else "<work-dir>", result["context"]
            )


class ProgressTests(LocalFiles):
    def test_phase_local_progress_reset_and_committed_terminal(self):
        output = self.root / "archive.dpack"
        progress = acceptance.ProgressSummary("compress", 100, output)
        progress(event())
        progress(event(completed_bytes=100, completed_items=10, percentage=100))
        progress(event(stage="writing", completed_bytes=1, completed_items=0))
        output.write_bytes(b"committed")
        progress(terminal())
        progress.require_success()
        self.assertEqual(progress.report()["event_count"], 4)
        self.assertTrue(progress.report()["committed_output_at_terminal"])

    def test_unknown_total_remains_indeterminate(self):
        progress = acceptance.ProgressSummary("compress", 100)
        progress(event(total_bytes=None, percentage=None))
        progress(terminal())
        progress.require_success()

    def test_counter_regression_and_implausible_events_fail(self):
        for bad, violation in (
            (event(completed_bytes=1), "phase_counter_regressed"),
            (event(completed_items=0), "phase_counter_regressed"),
            (event(completed_bytes=101), "implausible_completed_bytes"),
            (event(total_bytes=1000), "implausible_byte_total"),
            (event(percentage=float("nan")), "invalid_percentage"),
            (event(total_bytes=None), "percentage_without_total"),
            (event(operation="analyze"), "wrong_operation"),
        ):
            with self.subTest(violation=violation):
                progress = acceptance.ProgressSummary("compress", 100)
                progress(event())
                progress(bad)
                progress(terminal())
                self.assertIn(violation, progress.violations)
                with self.assertRaises(acceptance.AcceptanceError):
                    progress.require_success()

    def test_missing_duplicate_and_post_terminal_events_fail(self):
        for events in ((event(),), (terminal(), terminal()), (terminal(), event())):
            progress = acceptance.ProgressSummary("compress", 100)
            for snapshot in events:
                progress(snapshot)
            with self.assertRaises(acceptance.AcceptanceError):
                progress.require_success()

    def test_terminal_requires_committed_output(self):
        progress = acceptance.ProgressSummary("compress", 100, self.work)
        progress(terminal())
        self.assertIn("terminal_before_committed_output", progress.violations)

    def test_progress_storage_is_bounded(self):
        progress = acceptance.ProgressSummary("compress", 100)
        for _ in range(10000):
            progress(event())
        self.assertEqual(progress.count, 10000)
        self.assertLess(len(json.dumps(progress.report())), 1000)

    def test_cancellation_evidence_requires_real_request_error_and_clean_transaction(
        self,
    ):
        progress = acceptance.ProgressSummary("compress", 100)
        progress(event())
        error = SimpleNamespace(code="cancelled", category="cancellation")
        evidence = acceptance.cancellation_evidence(10, error, progress, self.work, [])
        self.assertTrue(evidence["passed"])
        for requested, caught, unexpected in (
            (None, error, []),
            (10, None, []),
            (10, error, ["partial"]),
        ):
            self.assertFalse(
                acceptance.cancellation_evidence(
                    requested, caught, progress, self.work, unexpected
                )["passed"]
            )
        progress(terminal())
        self.assertFalse(
            acceptance.cancellation_evidence(10, error, progress, self.work, [])[
                "passed"
            ]
        )


class PublicSdkIntegrationTests(LocalFiles):
    def setUp(self):
        super().setUp()
        try:
            self.sdk, self.version = acceptance.load_sdk()
        except ImportError:
            self.skipTest("Install datapack-engine to run public SDK integration tests")
        # Exercise the real SDK with tiny inputs, independently of CI disk capacity.
        patched_space = mock.patch.object(
            acceptance.shutil, "disk_usage", return_value=SimpleNamespace(free=10**12)
        )
        patched_space.start()
        self.addCleanup(patched_space.stop)

    def run_tool(self, *extra):
        with contextlib.redirect_stdout(io.StringIO()):
            return acceptance.execute(self.args(*extra))

    def test_full_public_sdk_roundtrip_cancellation_retry_and_report_schema(self):
        original = self.source.read_bytes()
        report, code = self.run_tool("--cancel-retry", "--keep-output")
        self.assertEqual(code, 0, report.get("error"))
        self.assertEqual(report["schema_version"], 1)
        self.assertEqual(report["status"], "passed")
        self.assertTrue(report["acceptance_completed"])
        for key in (
            "environment",
            "input",
            "analysis",
            "configuration",
            "compression",
            "operations",
            "cancellation",
            "validation",
            "decompression",
            "independent_verification",
            "timing_disclaimer",
            "preflight",
        ):
            self.assertIn(key, report)
        self.assertTrue(report["cancellation"]["passed"])
        self.assertTrue(report["cancellation"]["retry_succeeded"])
        self.assertFalse(report["cancellation"]["false_terminal_success"])
        self.assertEqual(report["compression"]["archive_version"], 2)
        self.assertEqual(report["compression"]["chunk_count"], 1)
        self.assertTrue(report["validation"]["valid"])
        self.assertTrue(report["decompression"]["verified"])
        self.assertTrue(report["independent_verification"]["byte_for_byte_match"])
        self.assertEqual((self.work / "restored.csv").read_bytes(), original)
        self.assertEqual(self.source.read_bytes(), original)
        for operation in report["operations"].values():
            self.assertEqual(operation["progress"]["terminal_count"], 1)
            self.assertEqual(operation["progress"]["violations"], [])
        saved = (self.work / acceptance.REPORT_NAME).read_text()
        self.assertNotIn(str(self.root), saved)
        self.assertEqual(json.loads(saved), report)

    def test_explicit_cleanup_preserves_report_and_source(self):
        report, code = self.run_tool("--cleanup-output")
        self.assertEqual(code, 0, report.get("error"))
        self.assertFalse(report["outputs_retained"])
        self.assertEqual(
            list(self.work.iterdir()), [self.work / acceptance.REPORT_NAME]
        )
        self.assertTrue(self.source.exists())

    def test_preflight_profile_never_claims_full_acceptance(self):
        report, code = self.run_tool("--profile", "preflight")
        self.assertEqual(code, 0)
        self.assertFalse(report["acceptance_completed"])
        self.assertEqual(report["operations"], {})
        self.assertEqual(report["artifacts"], {})

    def test_expected_hash_mismatch_refuses_before_workspace_mutation(self):
        with self.assertRaisesRegex(acceptance.AcceptanceError, "SHA-256 differs"):
            self.run_tool("--expected-sha256", "0" * 64)
        self.assertFalse(self.work.exists())

    def test_space_drop_during_preflight_refuses_before_workspace_mutation(self):
        with mock.patch.object(
            acceptance.shutil,
            "disk_usage",
            side_effect=[SimpleNamespace(free=10**12), SimpleNamespace(free=0)],
        ):
            with self.assertRaisesRegex(
                acceptance.AcceptanceError, "during source hashing"
            ):
                self.run_tool()
        self.assertFalse(self.work.exists())

    def test_unexpected_cancellation_error_keeps_evidence_and_stable_error_identity(
        self,
    ):
        failure = self.sdk.DataPackOutputError(f"Output failed at {self.work}")
        with mock.patch.object(self.sdk, "compress", side_effect=failure):
            report, code = self.run_tool("--cancel-retry")
        self.assertEqual(code, 1)
        self.assertEqual(report["stage"], "cancellation")
        self.assertFalse(report["cancellation"]["passed"])
        self.assertFalse(report["cancellation"]["requested"])
        self.assertEqual(report["error"]["code"], "output_error")
        self.assertEqual(report["error"]["category"], "output")
        self.assertIn("<work-dir>", report["error"]["context"])

    def test_invalid_validation_result_fails_and_retains_archive_even_with_cleanup(
        self,
    ):
        original_validate = self.sdk.validate

        def invalid(*args, **kwargs):
            result = original_validate(*args, **kwargs)
            result["valid"] = False
            return result

        with mock.patch.object(self.sdk, "validate", side_effect=invalid):
            report, code = self.run_tool("--cleanup-output")
        self.assertEqual(code, 1)
        self.assertEqual(report["stage"], "validate")
        self.assertFalse(report["validation"]["valid"])
        self.assertFalse(report["acceptance_completed"])
        self.assertTrue((self.work / "archive.dpack").exists())
        self.assertFalse((self.work / "restored.csv").exists())
        self.assertTrue(report["outputs_retained"])

    def test_operation_failure_keeps_failed_report_without_false_success(self):
        failure = OSError(f"Cannot access {self.source}")
        with mock.patch.object(self.sdk, "analyze", side_effect=failure):
            report, code = self.run_tool()
        self.assertEqual(code, 1)
        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["stage"], "analyze")
        self.assertFalse(report["acceptance_completed"])
        self.assertEqual(
            report["operations"]["analyze"]["progress"]["terminal_count"], 0
        )
        self.assertIn("<input>", report["error"]["context"])
        self.assertFalse((self.work / "archive.dpack").exists())
        self.assertEqual(
            json.loads((self.work / acceptance.REPORT_NAME).read_text()), report
        )


if __name__ == "__main__":
    unittest.main()
