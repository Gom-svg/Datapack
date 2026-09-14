"""Focused package safety checks; synthetic PE headers are not Windows evidence."""

import struct
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import desktop_candidate as desktop  # noqa: E402


def pe():
    data = bytearray(256)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 60, 64)
    data[64:68] = b"PE\0\0"
    struct.pack_into("<H", data, 68, 0x8664)
    struct.pack_into("<H", data, 88, 0x20B)
    struct.pack_into("<H", data, 156, 2)
    return bytes(data)


class DesktopPackageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def package(self, extra=None):
        files = {name: b"documentation" for name in desktop.MEMBERS}
        files["datapack-desktop.exe"] = pe()
        files["demo.csv"] = (
            desktop.ROOT / "tests/fixtures/desktop/demo.csv"
        ).read_bytes()
        if extra:
            files.update(extra)
        path = self.root / desktop.PACKAGE
        with zipfile.ZipFile(path, "w") as archive:
            for name, data in files.items():
                archive.writestr(name, data)
        return path

    def test_expected_package_is_accepted(self):
        desktop.inspect_package(self.package())

    def test_unexpected_file_and_traversal_rejected(self):
        for name in ("secret.txt", "../escape", "folder/file"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                desktop.inspect_package(self.package({name: b"unwanted"}))

    def test_private_paths_are_rejected(self):
        with self.assertRaises(ValueError):
            desktop.inspect_package(
                self.package({"README.md": str(Path.home()).encode() + b"/private"})
            )

    def test_wrong_machine_and_console_subsystem_rejected(self):
        for offset, value in ((68, 0x14C), (88, 0x10B), (156, 3)):
            data = bytearray(pe())
            struct.pack_into("<H", data, offset, value)
            with self.assertRaises(ValueError):
                desktop.inspect_pe(data)

    def test_truncated_pe_rejected(self):
        with self.assertRaises(ValueError):
            desktop.inspect_pe(b"MZ")

    def test_changed_fixture_rejected(self):
        with self.assertRaises(ValueError):
            desktop.inspect_package(self.package({"demo.csv": b"real data"}))

    def test_duplicate_entry_rejected(self):
        path = self.package()
        with zipfile.ZipFile(path, "a") as archive:
            with self.assertWarns(UserWarning):
                archive.writestr("README.md", b"duplicate")
        with self.assertRaises(ValueError):
            desktop.inspect_package(path)

    def test_verify_detects_tampering(self):
        path = self.package()
        manifest = {
            "schema_version": 1,
            "version": desktop.VERSION,
            "product": "DataPack Desktop",
            "target": "x86_64-pc-windows-msvc",
            "source": {"commit": "a" * 40, "dirty": False},
            "rustc": "rustc 1.85.0 (synthetic)",
            "cargo": "cargo 1.85.0 (synthetic)",
            "lockfiles": {
                name: desktop.release.digest(desktop.ROOT / name)
                for name in ("Cargo.lock", "desktop/Cargo.lock")
            },
            "smokes": {
                "extracted_v1_v2_exact_bytes": True,
                "native_window_controls": True,
            },
            "package": {
                "name": desktop.PACKAGE,
                "members": sorted(desktop.MEMBERS),
                "bytes": path.stat().st_size,
                "sha256": desktop.release.digest(path),
            },
        }
        (self.root / desktop.MANIFEST).write_bytes(desktop.release.json_bytes(manifest))
        (self.root / desktop.SUMS).write_bytes(
            "".join(
                f"{desktop.release.digest(self.root / name)}  {name}\n"
                for name in (desktop.PACKAGE, desktop.MANIFEST)
            ).encode()
        )
        desktop.verify(self.root, "a" * 40)
        with self.assertRaises(ValueError):
            desktop.verify(self.root, "b" * 40)
        path.write_bytes(path.read_bytes() + b"tampered")
        with self.assertRaises(ValueError):
            desktop.verify(self.root)

    def test_version_and_dependency_inventory(self):
        desktop.check_version()


if __name__ == "__main__":
    unittest.main()
