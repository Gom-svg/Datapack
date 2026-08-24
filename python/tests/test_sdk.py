import sys
import tempfile
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
        self.assertIn("analyze failed", str(raised.exception))

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

        def failing_callback(_event):
            nonlocal calls
            calls += 1
            raise RuntimeError("progress callback sentinel")

        try:
            report = datapack.analyze(self.source, progress=failing_callback)
        finally:
            sys.unraisablehook = previous_hook

        self.assertEqual(report["schema_version"], 1)
        self.assertEqual(calls, 1)
        self.assertEqual(len(unraisable), 1)
        self.assertIsInstance(unraisable[0].exc_value, RuntimeError)
        self.assertIn("progress callback sentinel", str(unraisable[0].exc_value))

    def test_progress_must_be_callable(self) -> None:
        with self.assertRaises(datapack.DataPackConfigurationError):
            datapack.analyze(self.source, progress=object())


if __name__ == "__main__":
    unittest.main()
