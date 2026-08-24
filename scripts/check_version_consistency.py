#!/usr/bin/env python3
"""Verify DataPack product identity and mirrored release versions."""

from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parents[1]
SEMVER = re.compile(
    r"^(0|[1-9][0-9]*)\."
    r"(0|[1-9][0-9]*)\."
    r"(0|[1-9][0-9]*)"
    r"(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)


def fail(message: str) -> None:
    raise ValueError(message)


def section(path: Path, name: str) -> str:
    text = path.read_text(encoding="utf-8")
    match = re.search(
        rf"(?ms)^\[{re.escape(name)}\]\s*$\n(.*?)(?=^\[|\Z)", text
    )
    if match is None:
        fail(f"{path.relative_to(ROOT)}: missing [{name}] section")
    return match.group(1)


def string_value(path: Path, section_name: str, key: str) -> str:
    body = section(path, section_name)
    match = re.search(rf'(?m)^{re.escape(key)}\s*=\s*"([^"]+)"\s*$', body)
    if match is None:
        fail(f"{path.relative_to(ROOT)}: missing string {section_name}.{key}")
    return match.group(1)


def bool_value(path: Path, section_name: str, key: str) -> bool:
    body = section(path, section_name)
    match = re.search(rf"(?m)^{re.escape(key)}\s*=\s*(true|false)\s*$", body)
    if match is None:
        fail(f"{path.relative_to(ROOT)}: missing boolean {section_name}.{key}")
    return match.group(1) == "true"


def locked_versions(path: Path, package_name: str) -> set[str]:
    text = path.read_text(encoding="utf-8")
    versions = set()
    for package in re.split(r"(?m)^\[\[package\]\]\s*$", text)[1:]:
        name = re.search(r'(?m)^name\s*=\s*"([^"]+)"\s*$', package)
        version = re.search(r'(?m)^version\s*=\s*"([^"]+)"\s*$', package)
        if name is not None and version is not None and name.group(1) == package_name:
            versions.add(version.group(1))
    if not versions:
        fail(f"{path.relative_to(ROOT)}: package {package_name!r} was not found")
    return versions


def expect(actual: object, expected: object, description: str) -> None:
    if actual != expected:
        fail(f"{description}: expected {expected!r}, found {actual!r}")


def check() -> None:
    version = (ROOT / "VERSION").read_text(encoding="utf-8").strip()
    if SEMVER.fullmatch(version) is None:
        fail(f"VERSION is not a supported Semantic Version: {version!r}")

    core_manifest = ROOT / "Cargo.toml"
    binding_manifest = ROOT / "python" / "Cargo.toml"
    python_project = ROOT / "python" / "pyproject.toml"

    expect(string_value(core_manifest, "package", "name"), "datapack", "Rust package")
    expect(string_value(core_manifest, "package", "version"), version, "Rust version")
    expect(bool_value(core_manifest, "package", "publish"), False, "crates.io policy")
    expect(
        string_value(core_manifest, "package", "rust-version"),
        "1.85",
        "Rust MSRV",
    )

    expect(
        string_value(binding_manifest, "package", "name"),
        "datapack-python",
        "PyO3 crate",
    )
    expect(
        string_value(binding_manifest, "package", "version"),
        version,
        "PyO3 crate version",
    )
    expect(
        bool_value(binding_manifest, "package", "publish"),
        False,
        "PyO3 crates.io policy",
    )
    expect(
        string_value(binding_manifest, "package", "rust-version"),
        "1.85",
        "PyO3 Rust MSRV",
    )

    dependency_line = re.search(
        r'(?m)^datapack\s*=\s*\{([^}]*)\}\s*$',
        section(binding_manifest, "dependencies"),
    )
    if dependency_line is None:
        fail("python/Cargo.toml: missing exact root datapack dependency")
    dependency = dependency_line.group(1)
    dependency_version = re.search(r'version\s*=\s*"=([^"]+)"', dependency)
    dependency_path = re.search(r'path\s*=\s*"([^"]+)"', dependency)
    expect(
        dependency_version.group(1) if dependency_version else None,
        version,
        "PyO3 root dependency version",
    )
    expect(
        dependency_path.group(1) if dependency_path else None,
        "..",
        "PyO3 root dependency path",
    )

    expect(
        string_value(python_project, "project", "name"),
        "datapack-engine",
        "PyPI distribution",
    )
    expect(
        string_value(python_project, "project", "version"),
        version,
        "PyPI distribution version",
    )
    expect(
        string_value(python_project, "project", "requires-python"),
        ">=3.9",
        "Python runtime floor",
    )
    expect(
        string_value(python_project, "tool.maturin", "module-name"),
        "datapack._native",
        "Python native module",
    )

    init_text = (ROOT / "python/python/datapack/__init__.py").read_text(
        encoding="utf-8"
    )
    init_version = re.search(r'(?m)^__version__\s*=\s*"([^"]+)"\s*$', init_text)
    expect(
        init_version.group(1) if init_version else None,
        version,
        "Python import version",
    )

    cli_text = (ROOT / "src/cli/mod.rs").read_text(encoding="utf-8")
    if '#[command(name = "datapack")]' not in cli_text:
        fail("src/cli/mod.rs: CLI executable identity must remain 'datapack'")
    if not (ROOT / "python/python/datapack/__init__.py").is_file():
        fail("Python import package must remain python/python/datapack")

    expect(
        string_value(ROOT / "rust-toolchain.toml", "toolchain", "channel"),
        "1.85.0",
        "certified Rust toolchain",
    )

    expected_locks = (
        (ROOT / "Cargo.lock", "datapack"),
        (ROOT / "python/Cargo.lock", "datapack"),
        (ROOT / "python/Cargo.lock", "datapack-python"),
        (ROOT / "fuzz/Cargo.lock", "datapack"),
    )
    for lockfile, package_name in expected_locks:
        expect(
            locked_versions(lockfile, package_name),
            {version},
            f"{lockfile.relative_to(ROOT)} {package_name} version",
        )

    print(f"DataPack product identity and version consistency: PASS ({version})")


if __name__ == "__main__":
    try:
        check()
    except (OSError, ValueError) as error:
        print(f"version consistency: FAIL: {error}", file=sys.stderr)
        raise SystemExit(1) from error
