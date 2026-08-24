import sys
import tempfile
import threading
import unittest
from importlib.metadata import version
from pathlib import Path

import datapack


class DataPackSdkTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary_directory = tempfile.TemporaryDirectory()
        self.directory = Path(self.temporary_directory.name)
        self.source = self.directory / "source.csv"
        rows = ["id,kind,value\r\n"]
        rows.extend(f"{index},repeat,{index % 7:02d}\r\n" for index in range(512))
        self.original = "".join(rows).encode("utf-8")
        self.source.write_bytes(self.original)

    def tearDown(self) -> None:
        self.temporary_directory.cleanup()

    def test_distribution_identity_and_version_are_consistent(self) -> None:
        self.assertEqual(version("datapack-engine"), datapack.__version__)

    def test_v1_services_are_path_based_and_byte_exact(self) -> None:
        analysis = datapack.analyze(self.source, sample_mb=1)
        self.assertEqual(analysis["schema_version"], 1)
        self.assertEqual(analysis["dataset"]["source_size_bytes"], len(self.original))

        archive = self.directory / "source.dpack"
        compression = datapack.compress(self.source, archive)
        self.assertEqual(compression["archive_version"], 1)
        self.assertTrue(archive.is_file())

        validation = datapack.validate(archive, against=self.source)
        self.assertTrue(validation["valid"])
        self.assertEqual(validation["archive"]["format"], "dpack_v1")
        self.assertEqual(validation["against"]["status"], "matched")

        restored = self.directory / "restored.csv"
        decompression = datapack.decompress(archive, restored)
        self.assertEqual(decompression["archive_version"], 1)
        self.assertEqual(restored.read_bytes(), self.original)

        comparison = datapack.compare(self.source, mode="quick", runs=1)
        self.assertEqual(comparison["schema_version"], 1)
        self.assertEqual(comparison["methodology"]["runs"], 1)
        self.assertTrue(comparison["datapack"]["validation"]["sha256_match"])
        self.assertTrue(comparison["standalone_zstd"]["validation"]["sha256_match"])

    def test_v2_chunked_roundtrip_uses_the_same_rust_service(self) -> None:
        archive = self.directory / "source-v2.dpack"
        compression = datapack.compress(
            self.source,
            archive,
            options=datapack.V2CompressionOptions(
                chunk_size_bytes=1024,
                threads=1,
                max_in_flight_chunks=1,
            ),
        )
        self.assertEqual(compression["archive_version"], 2)

        validation = datapack.validate(archive, against=self.source)
        self.assertTrue(validation["valid"])
        self.assertEqual(validation["archive"]["format"], "dpack_v2")
        self.assertEqual(validation["checks"]["chunk_table"], "passed")
        self.assertEqual(validation["checks"]["per_chunk_sha256"], "passed")
        self.assertEqual(validation["checks"]["global_sha256"], "passed")

        restored = self.directory / "restored-v2.csv"
        decompression = datapack.decompress(archive, restored)
        self.assertEqual(decompression["archive_version"], 2)
        self.assertTrue(decompression["verified"])
        self.assertEqual(restored.read_bytes(), self.original)

    def test_rust_failures_map_to_the_custom_exception(self) -> None:
        missing = self.directory / "missing.csv"
        with self.assertRaises(datapack.DataPackAnalysisError) as raised:
            datapack.analyze(missing)
        self.assertIsInstance(raised.exception, datapack.DataPackError)
        self.assertEqual(raised.exception.category, "analysis")
        self.assertEqual(raised.exception.code, "analysis_error")
        self.assertIn("analyze failed", str(raised.exception))
        self.assertIn(str(missing), str(raised.exception))

    def test_exception_hierarchy_and_machine_identity_are_stable(self) -> None:
        expected = {
            datapack.DataPackIOError: ("io", "io_error"),
            datapack.DataPackFormatError: ("format", "format_error"),
            datapack.DataPackConfigurationError: (
                "configuration",
                "configuration_error",
            ),
            datapack.DataPackAnalysisError: ("analysis", "analysis_error"),
            datapack.DataPackOutputError: ("output", "output_error"),
            datapack.DataPackOperationError: ("operation", "operation_error"),
            datapack.DataPackTranslationError: (
                "translation",
                "translation_error",
            ),
            datapack.CancelledError: ("cancellation", "cancelled"),
        }
        for exception_type, (category, code) in expected.items():
            self.assertTrue(issubclass(exception_type, datapack.DataPackError))
            self.assertTrue(exception_type.__doc__)
            exception = exception_type("context")
            self.assertEqual(exception.category, category)
            self.assertEqual(exception.code, code)
            self.assertEqual(str(exception), "context")

    def test_public_docstrings_describe_results_controls_and_safe_outputs(self) -> None:
        expected = {
            datapack.analyze: ("AnalysisReport", "partial", "cancellation"),
            datapack.compress: (
                "CompressionResult",
                "overwrite",
                "transactional",
            ),
            datapack.decompress: (
                "DecompressionResult",
                "verification",
                "overwrite",
            ),
            datapack.validate: ("ValidationReport", "valid=False", "against"),
            datapack.compare: ("ComparisonReport", "quick", "Winners"),
            datapack.CancellationToken: ("Thread-safe", "cooperative", "reset"),
            datapack.ProgressEvent: ("progress facts", "None", "successful"),
            datapack.V1CompressionOptions: (
                "Immutable",
                "planner-selected",
                "safety ceilings",
            ),
            datapack.V2CompressionOptions: (
                "Immutable",
                "bounded",
                "process-RSS ceiling",
            ),
            datapack.DataPackError: ("Base exception", "DataPack SDK"),
            datapack.CancelledError: ("cooperatively cancelled",),
        }
        for public_object, phrases in expected.items():
            docstring = public_object.__doc__ or ""
            for phrase in phrases:
                self.assertIn(phrase, docstring)

    def test_invalidity_and_destination_conflict_are_not_ambiguous(self) -> None:
        malformed = self.directory / "malformed.dpack"
        malformed.write_bytes(b"not a DataPack archive")
        validation = datapack.validate(malformed)
        self.assertFalse(validation["valid"])
        self.assertTrue(validation["diagnostics"])

        archive = self.directory / "existing.dpack"
        archive.write_bytes(b"existing destination")
        with self.assertRaises(datapack.DataPackFormatError) as raised:
            datapack.compress(self.source, archive)
        self.assertEqual(raised.exception.category, "format")
        self.assertEqual(raised.exception.code, "format_error")
        self.assertIn(str(archive), str(raised.exception))
        self.assertIn("--force", str(raised.exception))
        self.assertEqual(archive.read_bytes(), b"existing destination")

    def test_invalid_options_map_to_the_custom_exception(self) -> None:
        with self.assertRaises(datapack.DataPackConfigurationError):
            datapack.V1CompressionOptions(mode="predictive")
        with self.assertRaises(datapack.DataPackConfigurationError):
            datapack.compare(self.source, mode="predictive")

    def test_optional_progress_callbacks_receive_typed_rust_facts(self) -> None:
        operations = {}

        def capture(name):
            events = []
            operations[name] = events
            return events.append

        datapack.analyze(self.source, sample_mb=1, progress=capture("analyze"))
        archive = self.directory / "progress.dpack"
        datapack.compress(
            self.source,
            archive,
            options=datapack.V2CompressionOptions(
                chunk_size_bytes=1024,
                threads=1,
                max_in_flight_chunks=1,
            ),
            progress=capture("compress"),
        )
        datapack.validate(archive, progress=capture("validate"))
        restored = self.directory / "progress-restored.csv"
        datapack.decompress(archive, restored, progress=capture("decompress"))
        datapack.compare(
            self.source,
            mode="quick",
            runs=1,
            progress=capture("compare"),
        )

        for operation, events in operations.items():
            self.assertTrue(events, operation)
            self.assertTrue(
                all(isinstance(event, datapack.ProgressEvent) for event in events)
            )
            self.assertTrue(all(event.operation == operation for event in events))
            self.assertEqual(events[-1].stage, "finalizing")
            self.assertEqual(events[-1].state, "completed")
            self.assertTrue(events[-1].terminal)
            self.assertIsNone(events[-1].total_bytes)
            self.assertIsNone(events[-1].percentage)
            for event in events:
                if event.total_bytes is not None:
                    self.assertLessEqual(event.completed_bytes, event.total_bytes)
                if event.total_items is not None:
                    self.assertLessEqual(event.completed_items, event.total_items)

        chunk_events = [
            event
            for event in operations["compress"]
            if event.stage == "compressing" and event.completed_items > 0
        ]
        self.assertTrue(chunk_events)
        self.assertEqual(chunk_events[-1].percentage, 100.0)
        self.assertEqual(
            chunk_events[-1].completed_items,
            chunk_events[-1].total_items,
        )
        self.assertEqual(restored.read_bytes(), self.original)

        with self.assertRaises(AttributeError):
            operations["analyze"][0].stage = "changed"

    def test_progress_callback_exception_is_isolated_and_reported_unraisable(
        self,
    ) -> None:
        unraisable = []
        previous_hook = sys.unraisablehook
        sys.unraisablehook = unraisable.append

        calls = 0
        token = datapack.CancellationToken()

        def failing_callback(_event):
            nonlocal calls
            calls += 1
            raise RuntimeError("progress callback sentinel")

        try:
            report = datapack.analyze(
                self.source,
                progress=failing_callback,
                cancellation=token,
            )
        finally:
            sys.unraisablehook = previous_hook

        self.assertEqual(report["schema_version"], 1)
        self.assertEqual(calls, 1)
        self.assertEqual(len(unraisable), 1)
        self.assertIsInstance(unraisable[0].exc_value, RuntimeError)
        self.assertIn("progress callback sentinel", str(unraisable[0].exc_value))
        self.assertFalse(token.is_cancelled)

    def test_cancellation_token_is_monotonic_shared_control(self) -> None:
        token = datapack.CancellationToken()
        self.assertFalse(token.is_cancelled)
        token.cancel()
        token.cancel()
        self.assertTrue(token.is_cancelled)
        with self.assertRaises(AttributeError):
            token.is_cancelled = False

    def test_pre_cancelled_operation_raises_typed_exception_without_output(
        self,
    ) -> None:
        token = datapack.CancellationToken()
        token.cancel()
        archive = self.directory / "pre-cancelled.dpack"

        with self.assertRaises(datapack.CancelledError) as raised:
            datapack.compress(self.source, archive, cancellation=token)

        self.assertIsInstance(raised.exception, datapack.DataPackError)
        self.assertEqual(raised.exception.category, "cancellation")
        self.assertEqual(raised.exception.code, "cancelled")
        self.assertFalse(archive.exists())

    def test_progress_callback_can_explicitly_cancel_v2(self) -> None:
        source = self.directory / "callback-cancel.bin"
        source.write_bytes(bytes(range(256)) * 8_192)
        archive = self.directory / "callback-cancel.dpack"
        token = datapack.CancellationToken()
        events = []

        def cancel_after_chunk(event):
            events.append(event)
            if event.stage == "compressing" and event.completed_items >= 1:
                token.cancel()

        with self.assertRaises(datapack.CancelledError):
            datapack.compress(
                source,
                archive,
                options=datapack.V2CompressionOptions(
                    chunk_size_bytes=64 * 1024,
                    threads=1,
                    max_in_flight_chunks=2,
                ),
                progress=cancel_after_chunk,
                cancellation=token,
            )

        self.assertTrue(token.is_cancelled)
        self.assertTrue(events)
        self.assertFalse(any(event.terminal for event in events))
        self.assertFalse(archive.exists())

    def test_another_python_thread_can_cancel_detached_rust_work(self) -> None:
        source = self.directory / "thread-cancel.bin"
        source.write_bytes(bytes(range(256)) * 8_192)
        archive = self.directory / "thread-source.dpack"
        datapack.compress(
            source,
            archive,
            options=datapack.V2CompressionOptions(
                chunk_size_bytes=64 * 1024,
                threads=1,
                max_in_flight_chunks=2,
            ),
        )
        restored = self.directory / "thread-restored.bin"
        token = datapack.CancellationToken()
        checkpoint_reached = threading.Event()
        cancellation_requested = threading.Event()

        def cancel_from_thread():
            if checkpoint_reached.wait(timeout=5):
                token.cancel()
                cancellation_requested.set()

        canceller = threading.Thread(target=cancel_from_thread)
        canceller.start()

        def coordinate(event):
            if event.stage == "decompressing" and event.completed_items >= 1:
                checkpoint_reached.set()
                self.assertTrue(cancellation_requested.wait(timeout=5))

        try:
            with self.assertRaises(datapack.CancelledError):
                datapack.decompress(
                    archive,
                    restored,
                    progress=coordinate,
                    cancellation=token,
                )
        finally:
            checkpoint_reached.set()
            canceller.join(timeout=5)

        self.assertFalse(canceller.is_alive())
        self.assertTrue(token.is_cancelled)
        self.assertFalse(restored.exists())

    def test_progress_must_be_callable(self) -> None:
        with self.assertRaises(datapack.DataPackConfigurationError):
            datapack.analyze(self.source, progress=object())


if __name__ == "__main__":
    unittest.main()
