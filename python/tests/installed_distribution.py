"""Tests that run only through the isolated installed-wheel certifier."""

import argparse
import filecmp
import hashlib
import importlib
import os
import shutil
import sys
import tempfile
import unittest
from importlib.metadata import distribution
from pathlib import Path

import datapack

EXPECTED_VERSION = ""
FORBIDDEN_SOURCE_ROOT = Path("/")


def is_within(path: Path, parent: Path) -> bool:
    try:
        path.resolve().relative_to(parent.resolve())
    except ValueError:
        return False
    return True


class InstalledDistributionTests(unittest.TestCase):
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

    def test_distribution_import_and_environment_are_isolated(self) -> None:
        installed = distribution("datapack-engine")
        native = importlib.import_module("datapack._native")
        package_path = Path(datapack.__file__).resolve()
        native_path = Path(native.__file__).resolve()
        environment_root = Path(sys.prefix).resolve()

        self.assertEqual(installed.metadata["Name"], "datapack-engine")
        self.assertEqual(installed.version, EXPECTED_VERSION)
        self.assertEqual(datapack.__version__, EXPECTED_VERSION)
        self.assertTrue(is_within(package_path, environment_root))
        self.assertTrue(is_within(native_path, environment_root))
        self.assertFalse(is_within(Path.cwd(), FORBIDDEN_SOURCE_ROOT))
        self.assertNotIn("PYTHONPATH", os.environ)

        for entry in sys.path:
            if entry:
                self.assertFalse(
                    is_within(Path(entry), FORBIDDEN_SOURCE_ROOT),
                    f"repository path leaked into sys.path: {entry}",
                )

        console_scripts = [
            entry
            for entry in installed.entry_points
            if entry.group == "console_scripts"
        ]
        self.assertEqual(console_scripts, [])
        self.assertIsNone(shutil.which("cargo"))
        self.assertIsNone(shutil.which("rustc"))
        self.assertIsNone(shutil.which("maturin"))

    def test_v1_public_sdk_workflow_is_byte_exact(self) -> None:
        analysis = datapack.analyze(self.source, sample_mb=1)
        self.assertEqual(analysis["schema_version"], 1)
        self.assertEqual(analysis["dataset"]["source_size_bytes"], len(self.original))

        archive = self.directory / "source.dpack"
        compression = datapack.compress(self.source, archive)
        self.assertEqual(compression["archive_version"], 1)

        validation = datapack.validate(archive, against=self.source)
        self.assertTrue(validation["valid"])
        self.assertEqual(validation["archive"]["format"], "dpack_v1")
        self.assertEqual(validation["against"]["status"], "matched")

        restored = self.directory / "restored.csv"
        decompression = datapack.decompress(archive, restored)
        self.assertEqual(decompression["archive_version"], 1)
        self.assertTrue(filecmp.cmp(self.source, restored, shallow=False))
        self.assertEqual(restored.read_bytes(), self.original)
        self.assertEqual(
            hashlib.sha256(restored.read_bytes()).digest(),
            hashlib.sha256(self.original).digest(),
        )

        comparison = datapack.compare(self.source, mode="quick", runs=1)
        self.assertEqual(comparison["schema_version"], 1)
        self.assertEqual(comparison["methodology"]["runs"], 1)
        self.assertTrue(comparison["datapack"]["validation"]["sha256_match"])
        self.assertTrue(comparison["standalone_zstd"]["validation"]["sha256_match"])

    def test_v2_installed_roundtrip_uses_native_engine(self) -> None:
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
        self.assertEqual(validation["checks"]["per_chunk_sha256"], "passed")
        self.assertEqual(validation["checks"]["global_sha256"], "passed")

        restored = self.directory / "restored-v2.csv"
        decompression = datapack.decompress(archive, restored)
        self.assertEqual(decompression["archive_version"], 2)
        self.assertTrue(decompression["verified"])
        self.assertEqual(restored.read_bytes(), self.original)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--expected-version", required=True)
    parser.add_argument("--forbidden-source-root", required=True, type=Path)
    arguments = parser.parse_args()

    global EXPECTED_VERSION
    global FORBIDDEN_SOURCE_ROOT
    EXPECTED_VERSION = arguments.expected_version
    FORBIDDEN_SOURCE_ROOT = arguments.forbidden_source_root.resolve()

    suite = unittest.defaultTestLoader.loadTestsFromTestCase(InstalledDistributionTests)
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main())
