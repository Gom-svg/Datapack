#!/usr/bin/env python3
"""Build and inspect an internal Windows Desktop candidate; never publish it."""

import argparse
import json
import os
import re
import struct
import sys
import tempfile
import zipfile
from pathlib import Path

import check_version_consistency as versions
import release_candidate as release

ROOT = release.ROOT
VERSION = release.VERSION
PACKAGE = f"datapack-desktop-{VERSION}-windows-x86_64.zip"
MANIFEST = "desktop-manifest.json"
SUMS = "SHA256SUMS.txt"
MEMBERS = {
    "datapack-desktop.exe",
    "README.md",
    "LICENSE-MIT",
    "THIRD-PARTY-LICENSES.txt",
    "demo.csv",
}


def check_version():
    versions.check()
    manifest = ROOT / "desktop/Cargo.toml"
    versions.expect(
        versions.string_value(manifest, "package", "version"),
        VERSION,
        "Desktop version",
    )
    versions.expect(
        versions.string_value(manifest, "package", "rust-version"),
        "1.85",
        "Desktop MSRV",
    )
    versions.expect(
        versions.bool_value(manifest, "package", "publish"),
        False,
        "Desktop publication",
    )
    for name in ("datapack", "datapack-desktop"):
        versions.expect(
            versions.locked_versions(ROOT / "desktop/Cargo.lock", name),
            {VERSION},
            f"Desktop lock {name}",
        )
    dependency = versions.section(manifest, "dependencies")
    release.require(
        f'version = "={VERSION}"' in dependency, "Desktop exact core version"
    )
    xml = (ROOT / "desktop/app.manifest").read_text(encoding="utf-8")
    release.require(f'version="{VERSION}.0"' in xml, "Windows manifest version")

    # The separate lock reuses every registry version/checksum from the frozen core.
    def packages(path):
        return {
            (
                re.search(r'^name = "([^"]+)"', block, re.M).group(1),
                re.search(r'^version = "([^"]+)"', block, re.M).group(1),
            ): re.search(r'^checksum = "([^"]+)"', block, re.M).group(1)
            for block in path.read_text().split("[[package]]")[1:]
            if 'source = "registry+' in block
        }

    core = packages(ROOT / "Cargo.lock")
    desktop = packages(ROOT / "desktop/Cargo.lock")
    release.require(
        all(core.get(key) == checksum for key, checksum in desktop.items()),
        "Desktop registry packages must match the certified core lock",
    )


def inspect_pe(data):
    release.require(len(data) >= 64 and data[:2] == b"MZ", "not a Windows executable")
    offset = struct.unpack_from("<I", data, 0x3C)[0]
    release.require(offset + 96 <= len(data), "truncated PE header")
    release.require(data[offset : offset + 4] == b"PE\0\0", "bad PE signature")
    release.require(
        struct.unpack_from("<H", data, offset + 4)[0] == 0x8664,
        "Desktop must be Windows x86_64",
    )
    release.require(
        struct.unpack_from("<H", data, offset + 24)[0] == 0x20B,
        "Desktop must use PE32+",
    )
    release.require(
        struct.unpack_from("<H", data, offset + 92)[0] == 2,
        "Desktop must use the Windows GUI subsystem",
    )


def inspect_package(path):
    with zipfile.ZipFile(path) as archive:
        entries = archive.infolist()
        release.require(
            len(entries) == len(MEMBERS), "unexpected/duplicate package entries"
        )
        release.require(
            {entry.filename for entry in entries} == MEMBERS,
            "unexpected Desktop package file set",
        )
        for entry in entries:
            release.require(
                not entry.is_dir() and entry.file_size <= 64 * 1024 * 1024,
                "unexpected package member size/type",
            )
            release.require(
                (entry.external_attr >> 16) & 0o170000 in (0, 0o100000),
                "non-regular package member",
            )
            release.scan(archive.read(entry))
        inspect_pe(archive.read("datapack-desktop.exe"))
        release.require(
            archive.read("demo.csv")
            == (ROOT / "tests/fixtures/desktop/demo.csv").read_bytes(),
            "unexpected demo fixture",
        )


def licenses():
    metadata = json.loads(
        release.output(
            [
                "cargo",
                "metadata",
                "--manifest-path",
                "desktop/Cargo.toml",
                "--locked",
                "--format-version",
                "1",
                "--filter-platform",
                "x86_64-pc-windows-msvc",
            ],
            cwd=ROOT,
        )
    )
    parts = [
        "DataPack Desktop dependency license inventory\nIncludes build/test dependencies in the platform graph.\n"
    ]
    for package in sorted(
        metadata["packages"], key=lambda p: (p["name"], p["version"])
    ):
        if package["source"] is None:
            continue
        folder = Path(package["manifest_path"]).parent
        files = sorted(
            p
            for p in folder.iterdir()
            if p.is_file() and p.name.lower().startswith(("license", "copying"))
        )
        release.require(bool(files), f"missing license text: {package['name']}")
        parts.append(
            f"\n{package['name']} {package['version']} — {package['license']}\n"
        )
        for path in files:
            parts.append(path.name + "\n" + path.read_text(encoding="utf-8"))
    return "\n".join(parts).encode("utf-8")


def build(out, allow_dirty=False):
    release.require(
        sys.platform == "win32", "candidate certification requires native Windows"
    )
    check_version()
    source = release.source_identity(allow_dirty=allow_dirty)
    rust = release.output(["rustc", "--version"])
    cargo = release.output(["cargo", "--version"])
    release.require(
        rust.startswith("rustc 1.85.0 ") and cargo.startswith("cargo 1.85.0 "),
        "certified Rust/Cargo 1.85.0 required",
    )
    out.mkdir(parents=True, exist_ok=False)
    environment = release.build_environment()
    release.run(
        [
            "cargo",
            "build",
            "--manifest-path",
            "desktop/Cargo.toml",
            "--release",
            "--locked",
            "--target",
            "x86_64-pc-windows-msvc",
        ],
        cwd=ROOT,
        env=environment,
    )
    metadata = json.loads(
        release.output(
            [
                "cargo",
                "metadata",
                "--manifest-path",
                "desktop/Cargo.toml",
                "--format-version",
                "1",
                "--no-deps",
                "--locked",
            ],
            cwd=ROOT,
        )
    )
    binary = (
        Path(metadata["target_directory"])
        / "x86_64-pc-windows-msvc/release/datapack-desktop.exe"
    )
    members = {
        "datapack-desktop.exe": binary.read_bytes(),
        "README.md": (ROOT / "desktop/README.md").read_bytes(),
        "LICENSE-MIT": (ROOT / "LICENSE-MIT").read_bytes(),
        "demo.csv": (ROOT / "tests/fixtures/desktop/demo.csv").read_bytes(),
        "THIRD-PARTY-LICENSES.txt": licenses(),
    }
    with zipfile.ZipFile(
        out / PACKAGE, "w", compression=zipfile.ZIP_DEFLATED
    ) as archive:
        for name, content in sorted(members.items()):
            entry = zipfile.ZipInfo(name, date_time=(2020, 1, 1, 0, 0, 0))
            entry.external_attr = 0o100644 << 16
            entry.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(entry, content)
    inspect_package(out / PACKAGE)
    with tempfile.TemporaryDirectory(prefix="datapack-desktop-") as temporary:
        root = Path(temporary)
        with zipfile.ZipFile(out / PACKAGE) as archive:
            archive.extractall(root)  # Exact flat allowlist already inspected.
        executable = root / "datapack-desktop.exe"
        release.run([str(executable), "--smoke-test", str(root / "smoke")], timeout=120)
        release.require(
            (root / "smoke/SMOKE-PASSED.txt").is_file(), "missing adapter smoke marker"
        )
        release.run([str(executable), "--ui-smoke"], timeout=30)
    manifest = {
        "schema_version": 1,
        "product": "DataPack Desktop",
        "version": VERSION,
        "target": "x86_64-pc-windows-msvc",
        "source": source,
        "rustc": rust,
        "cargo": cargo,
        "lockfiles": {
            name: release.digest(ROOT / name)
            for name in ("Cargo.lock", "desktop/Cargo.lock")
        },
        "workflow": {
            name: os.environ.get(name)
            for name in ("GITHUB_WORKFLOW", "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT")
        },
        "smokes": {"extracted_v1_v2_exact_bytes": True, "native_window_controls": True},
        "package": {
            "name": PACKAGE,
            "bytes": (out / PACKAGE).stat().st_size,
            "sha256": release.digest(out / PACKAGE),
            "members": sorted(MEMBERS),
        },
        "status": "local dirty-source rehearsal"
        if source["dirty"]
        else "internal candidate; operator visual acceptance pending",
    }
    (out / MANIFEST).write_bytes(release.json_bytes(manifest))
    (out / SUMS).write_bytes(
        "".join(
            f"{release.digest(out / name)}  {name}\n" for name in (PACKAGE, MANIFEST)
        ).encode("utf-8")
    )
    verify(out)


def verify(folder, expected_commit=None):
    release.require(
        {p.name for p in folder.iterdir()} == {PACKAGE, MANIFEST, SUMS},
        "unexpected candidate file set",
    )
    manifest = json.loads((folder / MANIFEST).read_bytes())
    release.require(
        manifest["schema_version"] == 1 and manifest["version"] == VERSION,
        "unexpected Desktop manifest version",
    )
    release.require(
        manifest["product"] == "DataPack Desktop"
        and manifest["target"] == "x86_64-pc-windows-msvc",
        "unexpected product/target",
    )
    source = manifest["source"]
    release.require(
        re.fullmatch(r"[0-9a-f]{40}", source["commit"]) is not None
        and type(source["dirty"]) is bool,
        "invalid source provenance",
    )
    if expected_commit is not None:
        release.require(
            source["commit"] == expected_commit and not source["dirty"],
            "candidate does not match expected clean source",
        )
    release.require(
        manifest["rustc"].startswith("rustc 1.85.0 ")
        and manifest["cargo"].startswith("cargo 1.85.0 "),
        "unexpected toolchain",
    )
    release.require(
        manifest["lockfiles"]
        == {
            name: release.digest(ROOT / name)
            for name in ("Cargo.lock", "desktop/Cargo.lock")
        },
        "lockfile provenance differs from this checkout",
    )
    release.require(
        manifest["smokes"]
        == {"extracted_v1_v2_exact_bytes": True, "native_window_controls": True},
        "incomplete Desktop smoke record",
    )
    package = manifest["package"]
    release.require(
        package["name"] == PACKAGE and package["members"] == sorted(MEMBERS),
        "unexpected package identity",
    )
    release.require(
        package["bytes"] == (folder / PACKAGE).stat().st_size
        and package["sha256"] == release.digest(folder / PACKAGE),
        "package digest mismatch",
    )
    expected = "".join(
        f"{release.digest(folder / name)}  {name}\n" for name in (PACKAGE, MANIFEST)
    )
    release.require(
        (folder / SUMS).read_text(encoding="utf-8") == expected, "checksum mismatch"
    )
    inspect_package(folder / PACKAGE)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("check-version")
    build_parser = sub.add_parser("build")
    build_parser.add_argument("--out", type=Path, required=True)
    build_parser.add_argument("--allow-dirty", action="store_true")
    verify_parser = sub.add_parser("verify")
    verify_parser.add_argument("directory", type=Path)
    verify_parser.add_argument("--expected-commit")
    args = parser.parse_args()
    if args.command == "build":
        build(args.out.resolve(), args.allow_dirty)
    elif args.command == "verify":
        verify(args.directory, args.expected_commit)
    else:
        check_version()
    print(f"Desktop {args.command}: PASS")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError) as error:
        raise SystemExit(f"Desktop certification failed: {error}") from error
