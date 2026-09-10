"""Contract/failure tests. Synthetic headers never count as native certification."""

import hashlib
import io
import json
import os
import struct
import sys
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import release_candidate as rc  # noqa: E402


def binary_bytes(platform):
    data = bytearray(256)
    if platform == "linux":
        data[:6] = b"\x7fELF\x02\x01"
        struct.pack_into("<H", data, 18, 62)
    else:
        data[:2] = b"MZ"
        struct.pack_into("<I", data, 60, 128)
        data[128:134] = b"PE\0\0\x64\x86"
    return bytes(data)


class ArtifactWorkspace(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def package(self, platform, directory=None):
        directory = directory or self.root
        directory.mkdir(parents=True, exist_ok=True)
        binary = self.root / (platform + "-input")
        binary.write_bytes(binary_bytes(platform))
        package = directory / rc.cli_name(platform)
        rc.package_cli(binary, package, platform)
        return package


class PackagingTests(ArtifactWorkspace):
    def test_both_archive_formats_exact_members_and_deterministic_packaging(self):
        for platform in rc.TARGETS:
            with self.subTest(platform=platform):
                first = self.package(platform, self.root / platform / "first")
                second = self.package(platform, self.root / platform / "second")
                self.assertEqual(first.read_bytes(), second.read_bytes())
                binary = rc.inspect_cli(
                    first, platform, self.root / platform / "extract"
                )
                self.assertEqual(binary.read_bytes(), binary_bytes(platform))
                self.assertEqual(
                    {p.name for p in binary.parent.iterdir()},
                    {*rc.documents(), rc.executable_name(platform)},
                )

    def test_wrong_filename_is_rejected(self):
        package = self.package("linux")
        wrong = package.with_name("wrong.tar.gz")
        package.rename(wrong)
        with self.assertRaisesRegex(ValueError, "filename"):
            rc.inspect_cli(wrong, "linux")

    def test_wrong_architecture_is_rejected(self):
        data = bytearray(binary_bytes("linux"))
        struct.pack_into("<H", data, 18, 183)
        with self.assertRaisesRegex(ValueError, "x86_64"):
            rc.inspect_binary(bytes(data), "linux")
        with self.assertRaises(ValueError):
            rc.inspect_binary(binary_bytes("linux"), "windows")

    def test_traversal_link_duplicate_and_missing_tar_members_rejected(self):
        for attack in ("traversal", "symlink", "duplicate", "missing"):
            with self.subTest(attack=attack):
                package = self.root / rc.cli_name("linux")
                with tarfile.open(package, "w:gz") as archive:
                    item = tarfile.TarInfo(
                        "../outside"
                        if attack == "traversal"
                        else f"{rc.stem('linux')}/datapack"
                    )
                    item.mode = 0o755
                    if attack == "symlink":
                        item.type = tarfile.SYMTYPE
                        item.linkname = "../../outside"
                    else:
                        item.size = 1
                    archive.addfile(item, io.BytesIO(b"x"))
                    if attack == "duplicate":
                        archive.addfile(item, io.BytesIO(b"x"))
                with self.assertRaises(ValueError):
                    rc.inspect_cli(package, "linux", self.root / "extract")
                self.assertFalse((self.root / "outside").exists())

    def test_zip_unexpected_member_rejected_before_extraction(self):
        package = self.package("windows")
        with zipfile.ZipFile(package, "a") as archive:
            archive.writestr("../outside", b"x")
        with self.assertRaises(ValueError):
            rc.inspect_cli(package, "windows", self.root / "extract")
        self.assertFalse((self.root / "outside").exists())

    def test_document_tampering_rejected(self):
        package = self.root / rc.cli_name("windows")
        with zipfile.ZipFile(package, "w") as archive:
            for name, data in {
                **rc.documents(),
                "datapack.exe": binary_bytes("windows"),
            }.items():
                item = zipfile.ZipInfo(f"{rc.stem('windows')}/{name}")
                item.create_system = 3
                item.external_attr = 0o100644 << 16
                archive.writestr(
                    item, b"wrong license" if name == "LICENSE-MIT" else data
                )
        with self.assertRaisesRegex(ValueError, "document"):
            rc.inspect_cli(package, "windows")

    def test_private_paths_and_sensitive_markers_rejected_in_both_encodings(self):
        for value in (
            "/home/example/build/file.rs",
            "C:\\Users\\example\\file.rs",
            "/tmp/build/file.rs",
            "D:\\a\\repo\\file.rs",
            "github_pat_example",
        ):
            for encoding in ("utf-8", "utf-16le"):
                with self.subTest(value=value, encoding=encoding):
                    with self.assertRaises(ValueError):
                        rc.scan(value.encode(encoding))

    def test_wheel_certifier_is_not_bypassed(self):
        path = self.root / rc.wheel_cert.expected_filename("linux")
        path.write_bytes(b"not a wheel")
        with self.assertRaises(zipfile.BadZipFile):
            rc.inspect_wheel(path, "linux")

    def test_standard_library_io_module_is_not_a_private_container_path(self):
        rc.scan(b"/rustc/compiler-hash/library/std/src/io/mod.rs")
        with self.assertRaises(ValueError):
            rc.scan(b"\0/io/src/main.rs")


class SourceAndBuildTests(unittest.TestCase):
    def test_dirty_checkout_requires_explicit_local_rehearsal(self):
        with patch.dict(os.environ, {}, clear=True):
            with patch.object(rc, "output", side_effect=["a" * 40, " M file"]):
                with self.assertRaisesRegex(ValueError, "dirty"):
                    rc.source_identity()
            with patch.object(rc, "output", side_effect=["a" * 40, " M file"]):
                self.assertTrue(rc.source_identity(allow_dirty=True)["dirty"])

    def test_hosted_rehearsal_and_wrong_checkout_sha_rejected(self):
        with patch.dict(os.environ, {"GITHUB_ACTIONS": "true"}, clear=True):
            with patch.object(rc, "output", side_effect=["a" * 40, ""]):
                with self.assertRaisesRegex(ValueError, "rehearsals"):
                    rc.source_identity(allow_dirty=True)
            os.environ["GITHUB_SHA"] = "b" * 40
            with patch.object(rc, "output", side_effect=["a" * 40, ""]):
                with self.assertRaisesRegex(ValueError, "SHA"):
                    rc.source_identity()

    def test_build_tools_use_invoked_python_environment_and_encoded_remapping(self):
        environment = rc.build_environment()
        self.assertEqual(
            environment["PATH"].split(os.pathsep)[0], str(Path(sys.executable).parent)
        )
        self.assertNotIn("RUSTFLAGS", environment)
        for value in (str(rc.ROOT), str(Path.home()), "/io", "/root"):
            self.assertIn(
                f"--remap-path-prefix={value}=", environment["CARGO_ENCODED_RUSTFLAGS"]
            )

    def test_reusable_workflow_records_actual_caller_identity(self):
        with patch.dict(
            os.environ,
            {
                "GITHUB_ACTIONS": "true",
                "GITHUB_WORKFLOW": "CI",
                "GITHUB_RUN_ID": "123",
                "GITHUB_RUN_ATTEMPT": "2",
            },
            clear=True,
        ):
            self.assertEqual(
                rc.workflow_identity(), {"name": "CI", "run_id": "123", "attempt": "2"}
            )


class BundleTests(ArtifactWorkspace):
    def setUp(self):
        super().setUp()
        self.source = {
            "commit": "a" * 40,
            "dirty": False,
            "lockfiles": {
                name: rc.digest(rc.ROOT / name)
                for name in ("Cargo.lock", "python/Cargo.lock")
            },
        }
        self.workflow = {"name": "local", "run_id": None, "attempt": None}
        for target, value in (
            ("source_identity", self.source),
            ("workflow_identity", self.workflow),
        ):
            mock = patch.object(rc, target, return_value=value)
            mock.start()
            self.addCleanup(mock.stop)
        # Wheels below are synthetic checksum inputs. Actual installation is a
        # separate native acceptance gate; do not execute these fixtures.
        mock = patch.object(rc, "inspect_wheel")
        self.wheel_inspection = mock.start()
        self.addCleanup(mock.stop)
        self.platforms = []
        for platform in rc.TARGETS:
            directory = self.root / platform
            package = self.package(platform, directory)
            wheel = directory / rc.wheel_cert.expected_filename(platform)
            wheel.write_bytes(b"synthetic checksum fixture")
            runtime = {
                "os": "Linux" if platform == "linux" else "Windows",
                "architecture": "x86_64",
            }
            if platform == "linux":
                runtime.update(glibc_symbol_floor="2.34", tested_glibc="2.39")
            receipt = {
                "schema_version": 1,
                "platform": platform,
                "version": rc.VERSION,
                "source": self.source,
                "workflow": self.workflow,
                "runtime": runtime,
                "toolchain": {"rustc": "1.85.0", "cargo": "1.85.0"},
                "artifacts": [
                    rc.artifact_entry(package, "cli", platform),
                    rc.artifact_entry(wheel, "wheel", platform),
                ],
            }
            (directory / rc.RECEIPT).write_bytes(rc.json_bytes(receipt))
            self.platforms.append(directory)
        self.bundle = self.root / "bundle"

    def assemble(self):
        rc.assemble(self.platforms, self.bundle, self.source["commit"])

    def test_complete_bundle_covers_four_artifacts_notes_and_manifest(self):
        self.assemble()
        self.assertEqual(len(list(self.bundle.iterdir())), 7)
        lines = (self.bundle / rc.SUMS).read_text().splitlines()
        self.assertEqual(len(lines), 6)
        for line in lines:
            checksum, name = line.split("  ")
            self.assertEqual(
                hashlib.sha256((self.bundle / name).read_bytes()).hexdigest(), checksum
            )
        self.assertEqual(self.wheel_inspection.call_count, 4)

    def test_missing_platform_fails_without_candidate_output(self):
        with self.assertRaisesRegex(ValueError, "both mandatory"):
            rc.assemble(self.platforms[:1], self.bundle, self.source["commit"])
        self.assertFalse(self.bundle.exists())

    def test_duplicate_platform_fails(self):
        with self.assertRaisesRegex(ValueError, "duplicate"):
            rc.assemble(
                [self.platforms[0], self.platforms[0]],
                self.bundle,
                self.source["commit"],
            )

    def test_tampered_platform_artifact_cannot_reuse_pass_record(self):
        (self.platforms[0] / rc.wheel_cert.expected_filename("linux")).write_bytes(
            b"tampered"
        )
        with self.assertRaisesRegex(ValueError, "smoke-tested"):
            self.assemble()
        self.assertFalse(self.bundle.exists())

    def test_wrong_commit_dirty_source_failed_smoke_and_workflow_rejected(self):
        path = self.platforms[0] / rc.RECEIPT
        original = path.read_bytes()
        for field in ("commit", "dirty", "smoke", "workflow", "lockfile"):
            receipt = json.loads(original)
            if field == "commit":
                receipt["source"]["commit"] = "b" * 40
            elif field == "dirty":
                receipt["source"]["dirty"] = True
            elif field == "smoke":
                receipt["artifacts"][0]["smoke"] = "FAIL"
            elif field == "lockfile":
                receipt["source"]["lockfiles"]["Cargo.lock"] = "0" * 64
            else:
                receipt["workflow"]["run_id"] = "42"
            path.write_bytes(rc.json_bytes(receipt))
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.assemble()
            self.assertFalse(self.bundle.exists())
        path.write_bytes(original)

    def test_manifest_and_checksum_tampering_fail_even_after_checksums_regenerated(
        self,
    ):
        self.assemble()
        path = self.bundle / rc.MANIFEST
        original = path.read_bytes()
        for field in ("size", "missing", "extra", "sha"):
            manifest = json.loads(original)
            if field == "size":
                manifest["artifacts"][0]["size_bytes"] += 1
            elif field == "missing":
                manifest["artifacts"].pop()
            elif field == "extra":
                manifest["unexpected"] = "value"
            else:
                manifest["source"]["commit"] = "b" * 40
            path.write_bytes(rc.json_bytes(manifest))
            (self.bundle / rc.SUMS).write_bytes(rc.checksum_bytes(self.bundle))
            with self.subTest(field=field), self.assertRaises(ValueError):
                rc.verify_bundle(self.bundle, self.source["commit"])
        path.write_bytes(original)
        (self.bundle / rc.SUMS).write_bytes(b"0" * 64 + b"  bad\n")
        with self.assertRaisesRegex(ValueError, "checksum"):
            rc.verify_bundle(self.bundle, self.source["commit"])

    def test_unexpected_bundle_file_rejected(self):
        self.assemble()
        (self.bundle / "build.log").write_text("not distributable")
        with self.assertRaisesRegex(ValueError, "file set"):
            rc.verify_bundle(self.bundle, self.source["commit"])

    @unittest.skipIf(
        os.name == "nt", "Windows symlink creation requires extra privileges"
    )
    def test_symlink_platform_artifact_rejected(self):
        wheel = self.platforms[0] / rc.wheel_cert.expected_filename("linux")
        original = self.root / "wheel-data"
        wheel.rename(original)
        wheel.symlink_to(original)
        with self.assertRaisesRegex(ValueError, "regular"):
            self.assemble()


if __name__ == "__main__":
    unittest.main()
