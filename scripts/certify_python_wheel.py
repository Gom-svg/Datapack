#!/usr/bin/env python3
"""Inspect and install-test one DataPack platform wheel."""

import argparse
import csv
import hashlib
import os
import re
import shutil
import subprocess
import sys
import tempfile
import venv
import zipfile
from email import policy
from email.parser import BytesParser
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[1]
VERSION = (ROOT / "VERSION").read_text(encoding="utf-8").strip()
DIST_INFO = f"datapack_engine-{VERSION}.dist-info"
PREFIX = f"datapack_engine-{VERSION}-cp39-abi3-"
INSTALLED_TEST = ROOT / "python/tests/installed_distribution.py"
ABSOLUTE_BUILD_PATH = re.compile(
    rb"(?:"
    rb"[A-Za-z]:[\\/]Users[\\/][^\\/\x00]+[\\/]"
    rb"|/mnt/[A-Za-z]/Users/[^/\x00]+/"
    rb"|/(?:home|Users)/[^/\x00]+/"
    rb"|/root/"
    rb")"
)
SENSITIVE_MARKERS = (
    b"-----BEGIN PRIVATE KEY-----",
    b"-----BEGIN OPENSSH PRIVATE KEY-----",
    b"github_pat_",
    b"ghp_",
)


def fail(message: str) -> None:
    raise ValueError(message)


def expected_filename(platform: str) -> str:
    if platform == "linux":
        return f"{PREFIX}manylinux_2_17_x86_64.manylinux2014_x86_64.whl"
    return f"{PREFIX}win_amd64.whl"


def expected_tags(platform: str) -> set[str]:
    if platform == "linux":
        return {
            "cp39-abi3-manylinux_2_17_x86_64",
            "cp39-abi3-manylinux2014_x86_64",
        }
    return {"cp39-abi3-win_amd64"}


def find_wheel(directory: Path, platform: str) -> Path:
    wheels = sorted(directory.resolve().glob("*.whl"))
    if len(wheels) != 1:
        fail(f"expected exactly one wheel in {directory}, found {len(wheels)}")
    wheel = wheels[0]
    expected = expected_filename(platform)
    if wheel.name != expected:
        fail(f"wheel filename: expected {expected!r}, found {wheel.name!r}")
    return wheel


def metadata_headers(archive: zipfile.ZipFile) -> dict[str, list[str]]:
    path = f"{DIST_INFO}/METADATA"
    message = BytesParser(policy=policy.default).parsebytes(archive.read(path))
    return {
        "name": message.get_all("Name", []),
        "version": message.get_all("Version", []),
        "summary": message.get_all("Summary", []),
        "requires_python": message.get_all("Requires-Python", []),
        "requires_dist": message.get_all("Requires-Dist", []),
        "license_expression": message.get_all("License-Expression", []),
        "license_file": message.get_all("License-File", []),
        "classifier": message.get_all("Classifier", []),
        "project_url": message.get_all("Project-URL", []),
    }


def inspect_wheel(wheel: Path, platform: str) -> None:
    with zipfile.ZipFile(wheel) as archive:
        names = archive.namelist()
        if len(names) != len(set(names)):
            fail("wheel contains duplicate archive members")

        for name in names:
            path = PurePosixPath(name)
            if path.is_absolute() or ".." in path.parts or "\\" in name:
                fail(f"wheel contains an unsafe member path: {name!r}")

        native_pattern = (
            re.compile(r"^datapack/_native\.abi3\.so$")
            if platform == "linux"
            else re.compile(r"^datapack/_native(?:\.abi3)?\.pyd$")
        )
        native = [name for name in names if native_pattern.fullmatch(name)]
        if len(native) != 1:
            fail(f"expected one native extension, found {native!r}")

        expected_members = {
            "datapack/__init__.py",
            "datapack/_native.pyi",
            "datapack/py.typed",
            native[0],
            f"{DIST_INFO}/METADATA",
            f"{DIST_INFO}/WHEEL",
            f"{DIST_INFO}/licenses/DATAPACK-LICENSE-MIT",
            f"{DIST_INFO}/RECORD",
        }
        unexpected = set(names) - expected_members
        missing = expected_members - set(names)
        if unexpected or missing:
            fail(
                "wheel contents differ from the runtime allowlist: "
                f"unexpected={sorted(unexpected)!r}, missing={sorted(missing)!r}"
            )

        headers = metadata_headers(archive)
        expected_headers = {
            "name": ["datapack-engine"],
            "version": [VERSION],
            "summary": ["Python bindings for the DataPack lossless compression engine"],
            "requires_python": [">=3.9"],
            "requires_dist": [],
            "license_expression": ["MIT"],
            "license_file": ["DATAPACK-LICENSE-MIT"],
            "project_url": ["Repository, https://github.com/Gom-svg/Datapack"],
        }
        for key, expected in expected_headers.items():
            if headers[key] != expected:
                fail(f"METADATA {key}: expected {expected!r}, found {headers[key]!r}")

        required_classifiers = {
            "Development Status :: 3 - Alpha",
            "Operating System :: Microsoft :: Windows",
            "Operating System :: POSIX :: Linux",
            "Programming Language :: Python :: 3.9",
            "Programming Language :: Python :: 3.10",
            "Programming Language :: Python :: 3.11",
            "Programming Language :: Python :: 3.12",
            "Programming Language :: Python :: 3.13",
            "Programming Language :: Python :: 3.14",
            "Programming Language :: Python :: Implementation :: CPython",
            "Programming Language :: Rust",
            "Typing :: Typed",
        }
        missing_classifiers = required_classifiers - set(headers["classifier"])
        if missing_classifiers:
            fail(f"METADATA is missing classifiers: {sorted(missing_classifiers)!r}")

        wheel_text = archive.read(f"{DIST_INFO}/WHEEL").decode("utf-8")
        tags = {
            line.removeprefix("Tag: ")
            for line in wheel_text.splitlines()
            if line.startswith("Tag: ")
        }
        if tags != expected_tags(platform):
            fail(f"wheel tags: expected {expected_tags(platform)!r}, found {tags!r}")
        if "Generator: maturin (1.14.1)" not in wheel_text:
            fail("wheel was not generated by the certified maturin 1.14.1")
        if "Root-Is-Purelib: false" not in wheel_text:
            fail("wheel incorrectly reports a pure-Python root")

        record_text = archive.read(f"{DIST_INFO}/RECORD").decode("utf-8")
        recorded = {row[0] for row in csv.reader(record_text.splitlines())}
        if recorded != set(names):
            fail("RECORD does not identify exactly the wheel members")

        packaged_license = archive.read(f"{DIST_INFO}/licenses/DATAPACK-LICENSE-MIT")
        if packaged_license != (ROOT / "LICENSE-MIT").read_bytes():
            fail("packaged MIT license differs from the repository license")

        for name in names:
            data = archive.read(name)
            path_match = ABSOLUTE_BUILD_PATH.search(data)
            if path_match is not None:
                leaked = path_match.group(0).decode("utf-8", errors="replace")
                fail(f"wheel member {name!r} leaks an absolute build path: {leaked}")
            for marker in SENSITIVE_MARKERS:
                if marker in data:
                    fail(f"wheel member {name!r} contains sensitive marker {marker!r}")

    digest = hashlib.sha256(wheel.read_bytes()).hexdigest()
    print(
        "Wheel artifact certification: PASS "
        f"({wheel.name}, {wheel.stat().st_size} bytes, sha256={digest})"
    )


def clean_environment(venv_python: Path) -> dict[str, str]:
    environment = os.environ.copy()
    for name in ("PYTHONPATH", "PYTHONHOME", "CARGO_HOME", "RUSTUP_HOME"):
        environment.pop(name, None)

    path_entries = [str(venv_python.parent)]
    if os.name == "nt":
        system_root = Path(environment.get("SystemRoot", r"C:\Windows"))
        path_entries.extend([str(system_root / "System32"), str(system_root)])
    else:
        path_entries.extend(["/usr/bin", "/bin"])
    environment["PATH"] = os.pathsep.join(path_entries)
    return environment


def install_test(wheel: Path) -> None:
    with tempfile.TemporaryDirectory(
        prefix="datapack-wheel-certification-"
    ) as temporary:
        workspace = Path(temporary).resolve()
        try:
            workspace.relative_to(ROOT.resolve())
        except ValueError:
            pass
        else:
            fail("temporary certification workspace is inside the repository")

        artifact_directory = workspace / "artifact"
        artifact_directory.mkdir()
        isolated_wheel = artifact_directory / wheel.name
        shutil.copy2(wheel, isolated_wheel)
        isolated_test = workspace / INSTALLED_TEST.name
        shutil.copy2(INSTALLED_TEST, isolated_test)

        environment_directory = workspace / "venv"
        venv.EnvBuilder(with_pip=True, clear=True).create(environment_directory)
        venv_python = (
            environment_directory / "Scripts/python.exe"
            if os.name == "nt"
            else environment_directory / "bin/python"
        )
        environment = clean_environment(venv_python)

        subprocess.run(
            [
                str(venv_python),
                "-I",
                "-m",
                "pip",
                "install",
                "--no-index",
                "--no-deps",
                "--disable-pip-version-check",
                str(isolated_wheel),
            ],
            check=True,
            cwd=workspace,
            env=environment,
        )
        subprocess.run(
            [
                str(venv_python),
                "-I",
                str(isolated_test),
                "--expected-version",
                VERSION,
                "--forbidden-source-root",
                str(ROOT.resolve()),
            ],
            check=True,
            cwd=workspace,
            env=environment,
        )
        version = subprocess.run(
            [str(venv_python), "--version"],
            check=True,
            capture_output=True,
            text=True,
            env=environment,
        ).stdout.strip()
        print(
            "Installed wheel certification: PASS "
            f"({version}; wheel-only install; repository absent from cwd/sys.path; "
            "Rust, Cargo, and maturin absent from PATH)"
        )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--wheel-dir", required=True, type=Path)
    parser.add_argument("--platform", required=True, choices=("linux", "windows"))
    parser.add_argument("--inspect-only", action="store_true")
    arguments = parser.parse_args()

    wheel = find_wheel(arguments.wheel_dir, arguments.platform)
    inspect_wheel(wheel, arguments.platform)
    if not arguments.inspect_only:
        install_test(wheel)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, zipfile.BadZipFile) as error:
        print(f"Python wheel certification: FAIL: {error}", file=sys.stderr)
        raise SystemExit(1) from error
