#!/usr/bin/env python3
"""Build, certify, assemble, and verify internal DataPack candidate artifacts."""

import argparse
import gzip
import hashlib
import io
import json
import os
import platform as host_platform
import re
import shutil
import stat
import struct
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path

import certify_python_wheel as wheel_cert
import check_version_consistency as versions

ROOT = Path(__file__).resolve().parents[1]
VERSION = (ROOT / "VERSION").read_text(encoding="utf-8").strip()
TARGETS = {
    "linux": "x86_64-unknown-linux-gnu",
    "windows": "x86_64-pc-windows-msvc",
}
NOTES = f"datapack-{VERSION}-RELEASE-NOTES.md"
MANIFEST = f"datapack-{VERSION}-manifest.json"
SUMS = f"datapack-{VERSION}-SHA256SUMS.txt"
RECEIPT = "certification.json"
EXTRA_PATHS = re.compile(
    rb"(?:/(?:tmp|var/tmp)/|[A-Za-z]:[\\/]a[\\/]|(?<![A-Za-z0-9_./\\-])/io/)"
)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def run(command, **kwargs):
    return subprocess.run(command, check=True, **kwargs)


def output(command, **kwargs):
    return run(command, capture_output=True, text=True, **kwargs).stdout.strip()


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def json_bytes(value):
    return (
        json.dumps(value, indent=2, sort_keys=True, ensure_ascii=True) + "\n"
    ).encode()


def scan(data):
    # Keep the P2 policy and additionally reject temporary/runner checkout paths.
    for content in (data, data.replace(b"\0", b"")):
        require(
            not wheel_cert.ABSOLUTE_BUILD_PATH.search(content), "private build path"
        )
        require(not EXTRA_PATHS.search(content), "temporary/runner build path")
        for marker in wheel_cert.SENSITIVE_MARKERS:
            require(marker not in content, "sensitive marker in artifact")
        for path in (ROOT, Path.home(), Path(tempfile.gettempdir())):
            for spelling in (str(path), path.as_posix()):
                require((spelling + os.sep).encode() not in content, "local build path")


def build_environment():
    environment = os.environ.copy()
    environment["PATH"] = (
        str(Path(sys.executable).parent) + os.pathsep + environment.get("PATH", "")
    )
    mappings = [
        (str(ROOT), "datapack-src"),
        (str(Path.home()), "build-home"),
        (tempfile.gettempdir(), "build-temp"),
        ("/io", "datapack-src"),
        ("/root", "build-home"),
    ]
    flags = [f"--remap-path-prefix={source}={dest}" for source, dest in mappings]
    environment.pop("RUSTFLAGS", None)
    environment["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags)
    environment["CARGO_PROFILE_RELEASE_STRIP"] = "symbols"
    return environment


def configure_wheel():
    # Hosted paths have no spaces. The local build uses encoded flags instead.
    environment = build_environment()
    flags = environment["CARGO_ENCODED_RUSTFLAGS"].split("\x1f")
    require(
        all(not any(c.isspace() for c in flag) for flag in flags),
        "hosted wheel remapping requires paths without whitespace",
    )
    with Path(os.environ["GITHUB_ENV"]).open("a", encoding="utf-8") as stream:
        stream.write("RUSTFLAGS=" + " ".join(flags) + "\n")


def source_identity(allow_dirty=False):
    commit = output(["git", "rev-parse", "HEAD"], cwd=ROOT)
    require(re.fullmatch(r"[0-9a-f]{40}", commit), "invalid source SHA")
    dirty = bool(
        output(["git", "status", "--porcelain", "--untracked-files=all"], cwd=ROOT)
    )
    require(
        not dirty or allow_dirty,
        "source tree is dirty; use --allow-dirty only for a local rehearsal",
    )
    require(
        not (allow_dirty and os.environ.get("GITHUB_ACTIONS") == "true"),
        "dirty rehearsals are forbidden in hosted candidate builds",
    )
    if os.environ.get("GITHUB_SHA"):
        require(
            commit == os.environ["GITHUB_SHA"], "checkout SHA differs from workflow SHA"
        )
    return {
        "commit": commit,
        "dirty": dirty,
        "lockfiles": {
            name: digest(ROOT / name) for name in ("Cargo.lock", "python/Cargo.lock")
        },
    }


def workflow_identity():
    if os.environ.get("GITHUB_ACTIONS") == "true":
        run_id = os.environ["GITHUB_RUN_ID"]
        attempt = os.environ["GITHUB_RUN_ATTEMPT"]
        require(run_id.isdecimal() and attempt.isdecimal(), "invalid workflow identity")
        name = os.environ["GITHUB_WORKFLOW"]
        require(
            name in ("CI", "Release Candidate Artifacts"), "unexpected candidate caller"
        )
        return {
            "name": name,
            "run_id": run_id,
            "attempt": attempt,
        }
    return {"name": "local", "run_id": None, "attempt": None}


def stem(platform):
    return f"datapack-{VERSION}-{TARGETS[platform]}"


def cli_name(platform):
    return stem(platform) + (".tar.gz" if platform == "linux" else ".zip")


def executable_name(platform):
    return "datapack" if platform == "linux" else "datapack.exe"


def documents():
    return {
        "LICENSE-MIT": (ROOT / "LICENSE-MIT").read_bytes(),
        "SECURITY.md": (ROOT / "SECURITY.md").read_bytes(),
        "README.md": (ROOT / "docs/releases/CLI-PACKAGE-README.md")
        .read_bytes()
        .replace(b"@VERSION@", VERSION.encode()),
        "RELEASE-NOTES.md": (ROOT / "docs/releases/CANDIDATE-NOTES.md")
        .read_bytes()
        .replace(b"@VERSION@", VERSION.encode()),
    }


def inspect_binary(data, platform):
    if platform == "linux":
        require(
            data[:6] == b"\x7fELF\x02\x01" and len(data) >= 20,
            "CLI is not a 64-bit little-endian ELF",
        )
        require(struct.unpack_from("<H", data, 18)[0] == 62, "CLI is not x86_64 ELF")
    else:
        require(data[:2] == b"MZ" and len(data) >= 64, "CLI is not PE")
        offset = struct.unpack_from("<I", data, 60)[0]
        require(data[offset : offset + 6] == b"PE\0\0\x64\x86", "CLI is not x86_64 PE")
    scan(data)


def package_cli(binary, destination, platform):
    require(destination.name == cli_name(platform), "noncanonical CLI filename")
    members = documents()
    members[executable_name(platform)] = binary.read_bytes()
    inspect_binary(members[executable_name(platform)], platform)
    for data in members.values():
        scan(data)
    if platform == "linux":
        with destination.open("xb") as raw:
            with gzip.GzipFile(
                filename="", fileobj=raw, mode="wb", mtime=0
            ) as compressed:
                with tarfile.open(
                    fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT
                ) as archive:
                    for name, data in sorted(members.items()):
                        item = tarfile.TarInfo(f"{stem(platform)}/{name}")
                        item.size = len(data)
                        item.mode = (
                            0o755 if name == executable_name(platform) else 0o644
                        )
                        item.mtime = 0
                        archive.addfile(item, io.BytesIO(data))
    else:
        with zipfile.ZipFile(
            destination, "x", compression=zipfile.ZIP_DEFLATED
        ) as archive:
            for name, data in sorted(members.items()):
                item = zipfile.ZipInfo(
                    f"{stem(platform)}/{name}", (1980, 1, 1, 0, 0, 0)
                )
                item.create_system = 3
                item.external_attr = (stat.S_IFREG | 0o644) << 16
                item.compress_type = zipfile.ZIP_DEFLATED
                archive.writestr(item, data)


def inspect_cli(path, platform, extract_to=None):
    require(path.name == cli_name(platform), "noncanonical CLI filename")
    expected = {
        f"{stem(platform)}/{name}" for name in (*documents(), executable_name(platform))
    }
    members = {}
    if platform == "linux":
        with tarfile.open(path, "r:gz") as archive:
            for item in archive:
                require(
                    item.name in expected and item.name not in members,
                    "unexpected/duplicate TAR member",
                )
                require(
                    item.isfile() and not item.issparse() and not item.pax_headers,
                    "CLI archive must contain ordinary files only",
                )
                mode = 0o755 if item.name.endswith("/datapack") else 0o644
                require(
                    item.mode == mode
                    and item.uid == item.gid == item.mtime == 0
                    and item.uname == item.gname == "",
                    "nonnormalized TAR metadata",
                )
                require(0 <= item.size <= 128 * 1024 * 1024, "oversized CLI member")
                members[item.name] = archive.extractfile(item).read()
    else:
        with zipfile.ZipFile(path) as archive:
            for item in archive.infolist():
                require(
                    item.filename in expected and item.filename not in members,
                    "unexpected/duplicate ZIP member",
                )
                require(
                    item.external_attr >> 16 == stat.S_IFREG | 0o644
                    and item.date_time == (1980, 1, 1, 0, 0, 0)
                    and not item.extra
                    and not item.comment,
                    "nonnormalized ZIP metadata",
                )
                require(item.file_size <= 128 * 1024 * 1024, "oversized CLI member")
                members[item.filename] = archive.read(item)
    require(set(members) == expected, "missing CLI member")
    for name, data in members.items():
        scan(data)
        short = name.split("/")[1]
        if short in documents():
            require(data == documents()[short], "packaged document differs from source")
        else:
            inspect_binary(data, platform)
    # Never use extractall: write only the validated, exact allowlist.
    if extract_to is not None:
        for name, data in members.items():
            target = extract_to / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
        binary = extract_to / stem(platform) / executable_name(platform)
        binary.chmod(0o755)
        return binary
    return None


def smoke_cli(package, platform):
    with tempfile.TemporaryDirectory(prefix="datapack-cli-smoke-") as temporary:
        workspace = Path(temporary).resolve()
        require(
            ROOT != workspace and ROOT not in workspace.parents,
            "smoke workspace is inside checkout",
        )
        binary = inspect_cli(package, platform, workspace)
        environment = wheel_cert.clean_environment(binary)

        def invoke(*arguments, success=True):
            result = subprocess.run(
                [str(binary), *map(str, arguments)],
                cwd=workspace,
                env=environment,
                capture_output=True,
                timeout=60,
            )
            require(
                (result.returncode == 0) == success,
                f"packaged CLI command failed: {arguments[0]}",
            )
            return result.stdout

        version = invoke("--version").decode().strip()
        require(version == f"datapack {VERSION}", "CLI version mismatch")
        require(b"transactional" in invoke("--help").lower(), "missing CLI help")
        source = workspace / "sample.csv"
        # About 2 MiB, enough for a genuine multi-chunk CLI path (1 MiB minimum).
        source.write_bytes(
            b"id,category,value\r\n" + b'0001,"repeat",1.2500\r\n' * 110000
        )
        for wire in (1, 2):
            archive = workspace / f"sample-v{wire}.dpack"
            restored = workspace / f"result-v{wire}.csv"
            flags = (
                []
                if wire == 1
                else [
                    "--chunked",
                    "--chunk-size-mb",
                    "1",
                    "--threads",
                    "2",
                    "--max-in-flight-chunks",
                    "2",
                ]
            )
            invoke("compress", source, archive, *flags)
            validation = json.loads(
                invoke("validate", archive, "--against", source, "--json")
            )
            require(
                validation["valid"] and validation["archive"]["version"] == wire,
                "packaged archive validation/version failed",
            )
            if wire == 2:
                require(
                    validation["archive"]["chunk_count"] > 1,
                    "smoke did not exercise multiple chunks",
                )
            invoke("decompress", archive, restored)
            require(
                restored.read_bytes() == source.read_bytes()
                and digest(restored) == digest(source),
                "packaged CLI roundtrip mismatch",
            )
            invoke("compress", source, archive, *flags, success=False)
            invoke("decompress", archive, restored, success=False)
            require(
                restored.read_bytes() == source.read_bytes(),
                "overwrite modified destination",
            )
            broken = workspace / f"broken-v{wire}.dpack"
            broken.write_bytes(archive.read_bytes()[:16])
            absent = workspace / f"absent-v{wire}.csv"
            invoke("decompress", broken, absent, success=False)
            require(
                not absent.exists() and not list(workspace.glob("*.partial*")),
                "failure cleanup failed",
            )
        print(
            "Packaged CLI smoke: PASS (version/help, V1/V2 multichunk, exact bytes/SHA-256, overwrite, cleanup)"
        )
        return version


def artifact_entry(path, kind, platform=None):
    result = {
        "filename": path.name,
        "kind": kind,
        "target": TARGETS.get(platform),
        "size_bytes": path.stat().st_size,
        "sha256": digest(path),
    }
    if kind == "cli":
        result.update(cli_version=f"datapack {VERSION}", smoke="PASS")
    if kind == "wheel":
        result.update(
            distribution="datapack-engine",
            python_import="datapack",
            abi="cp39-abi3",
            tags=sorted(wheel_cert.expected_tags(platform)),
            smoke="PASS",
        )
    return result


def build_platform(destination, platform, wheel_directory=None, allow_dirty=False):
    require(not destination.exists(), "output directory must not already exist")
    require(
        host_platform.system() == ("Linux" if platform == "linux" else "Windows")
        and host_platform.machine().lower() in ("x86_64", "amd64"),
        "native x86_64 platform required",
    )
    versions.check()
    source = source_identity(allow_dirty)
    if wheel_directory is not None:
        require(
            os.environ.get("GITHUB_ACTIONS") == "true",
            "wheel reuse is restricted to the shared build in this hosted job",
        )
    for tool in ("rustc", "cargo"):
        require(
            output([tool, "--version"]).split()[:2] == [tool, "1.85.0"],
            "certified Rust/Cargo 1.85.0 required",
        )
    environment = build_environment()
    target = TARGETS[platform]
    run(
        [
            "cargo",
            "build",
            "--release",
            "--locked",
            "--target",
            target,
            "--bin",
            "datapack",
        ],
        cwd=ROOT,
        env=environment,
    )
    metadata = json.loads(
        output(
            ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"],
            cwd=ROOT,
        )
    )
    binary = (
        Path(metadata["target_directory"])
        / target
        / "release"
        / executable_name(platform)
    )
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="datapack-platform-") as temporary:
        staging = Path(temporary) / "platform"
        staging.mkdir()
        package = staging / cli_name(platform)
        package_cli(binary, package, platform)
        smoke_cli(package, platform)
        if wheel_directory is None:
            require(
                output([sys.executable, "-m", "maturin", "--version"])
                == "maturin 1.14.1",
                "maturin 1.14.1 required",
            )
            wheel_directory = Path(temporary) / "wheel"
            command = [
                sys.executable,
                "-m",
                "maturin",
                "build",
                "--manifest-path",
                "python/Cargo.toml",
                "--release",
                "--locked",
                "--strip",
                "--target",
                target,
                "--interpreter",
                sys.executable,
                "--out",
                str(wheel_directory),
            ]
            if platform == "linux":
                command += [
                    "--zig",
                    "--compatibility",
                    "manylinux2014",
                    "--auditwheel",
                    "check",
                ]
            run(command, cwd=ROOT, env=environment)
        wheel = wheel_cert.find_wheel(wheel_directory, platform)
        inspect_wheel(wheel, platform)
        wheel_cert.install_test(wheel)
        shutil.copyfile(wheel, staging / wheel.name)
        require(
            source_identity(allow_dirty) == source,
            "source identity changed during build",
        )
        runtime = {"os": host_platform.system(), "architecture": "x86_64"}
        if platform == "linux":
            glibc = re.findall(rb"GLIBC_(\d+\.\d+)", binary.read_bytes())
            require(glibc, "missing GNU libc version requirements")
            runtime["glibc_symbol_floor"] = max(
                glibc, key=lambda v: tuple(map(int, v.split(b".")))
            ).decode()
            runtime["tested_glibc"] = host_platform.libc_ver()[1]
        receipt = {
            "schema_version": 1,
            "platform": platform,
            "version": VERSION,
            "source": source,
            "workflow": workflow_identity(),
            "runtime": runtime,
            "toolchain": {"rustc": "1.85.0", "cargo": "1.85.0"},
            "artifacts": [
                artifact_entry(package, "cli", platform),
                artifact_entry(staging / wheel.name, "wheel", platform),
            ],
        }
        data = json_bytes(receipt)
        scan(data)
        (staging / RECEIPT).write_bytes(data)
        shutil.copytree(staging, destination)
    print(
        f"Platform artifact certification: PASS ({platform}; source_dirty={source['dirty']})"
    )


def directory_names(directory):
    require(
        directory.is_dir() and not directory.is_symlink(), "artifact directory required"
    )
    children = list(directory.iterdir())
    require(
        all(p.is_file() and not p.is_symlink() for p in children),
        "only regular artifact files allowed",
    )
    return {p.name for p in children}


def inspect_wheel(path, platform):
    require(
        path.name == wheel_cert.expected_filename(platform), "noncanonical wheel name"
    )
    wheel_cert.inspect_wheel(path, platform)
    with zipfile.ZipFile(path) as archive:
        for item in archive.infolist():
            scan(archive.read(item))


def manifest_data(directory, receipts, source, workflow):
    artifacts = []
    platforms = {}
    for platform in TARGETS:
        artifacts += [
            artifact_entry(directory / cli_name(platform), "cli", platform),
            artifact_entry(
                directory / wheel_cert.expected_filename(platform), "wheel", platform
            ),
        ]
        platforms[platform] = receipts[platform]["runtime"]
    artifacts.append(artifact_entry(directory / NOTES, "release-notes"))
    return {
        "schema_version": 1,
        "report_type": "datapack_release_candidate",
        "product": "DataPack",
        "version": VERSION,
        "channel": "internal-candidate",
        "source": source,
        "workflow": workflow,
        "toolchain": {"rustc": "1.85.0", "cargo": "1.85.0"},
        "python_abi": "cp39-abi3",
        "platforms": platforms,
        "artifacts": sorted(artifacts, key=lambda entry: entry["filename"]),
    }


def distributables():
    return {
        NOTES,
        *(cli_name(p) for p in TARGETS),
        *(wheel_cert.expected_filename(p) for p in TARGETS),
    }


def checksum_bytes(directory):
    return "".join(
        f"{digest(directory / name)}  {name}\n"
        for name in sorted(distributables() | {MANIFEST})
    ).encode()


def verify_bundle(directory, expected_commit):
    require(
        re.fullmatch(r"[0-9a-f]{40}", expected_commit),
        "full expected source SHA required",
    )
    require(
        directory_names(directory) == distributables() | {MANIFEST, SUMS},
        "candidate file set is incomplete or unexpected",
    )
    data = (directory / MANIFEST).read_bytes()
    scan(data)
    manifest = json.loads(data)
    require(data == json_bytes(manifest), "manifest JSON is not canonical")
    source = source_identity()
    require(
        source["commit"] == expected_commit and manifest["source"] == source,
        "source provenance mismatch; use the matching clean checkout",
    )
    workflow = manifest["workflow"]
    require(set(workflow) == {"name", "run_id", "attempt"}, "invalid workflow metadata")
    require(
        workflow == {"name": "local", "run_id": None, "attempt": None}
        or (
            workflow["name"] in ("CI", "Release Candidate Artifacts")
            and isinstance(workflow["run_id"], str)
            and workflow["run_id"].isdecimal()
            and isinstance(workflow["attempt"], str)
            and workflow["attempt"].isdecimal()
        ),
        "invalid workflow identity",
    )
    if os.environ.get("GITHUB_ACTIONS") == "true":
        require(workflow == workflow_identity(), "workflow provenance mismatch")
    require(
        (directory / NOTES).read_bytes() == documents()["RELEASE-NOTES.md"],
        "candidate notes differ from source",
    )
    receipts = {}
    for platform in TARGETS:
        inspect_cli(directory / cli_name(platform), platform)
        inspect_wheel(directory / wheel_cert.expected_filename(platform), platform)
        runtime = manifest["platforms"][platform]
        require(
            runtime["os"] == ("Linux" if platform == "linux" else "Windows")
            and runtime["architecture"] == "x86_64",
            "runtime platform mismatch",
        )
        if platform == "linux":
            require(
                set(runtime)
                == {"os", "architecture", "glibc_symbol_floor", "tested_glibc"}
                and all(
                    re.fullmatch(r"\d+\.\d+", runtime[k])
                    for k in ("glibc_symbol_floor", "tested_glibc")
                ),
                "invalid Linux runtime baseline",
            )
        else:
            require(
                set(runtime) == {"os", "architecture"}, "unexpected runtime metadata"
            )
        receipts[platform] = {"runtime": runtime}
    expected = manifest_data(directory, receipts, source, manifest["workflow"])
    require(
        manifest == expected, "manifest fields/size/hash/mandatory artifacts mismatch"
    )
    require(
        (directory / SUMS).read_bytes() == checksum_bytes(directory),
        "checksum set/order/digest mismatch",
    )
    print(
        "Candidate bundle verification: PASS (all mandatory artifacts, SHA-256, manifest, source provenance)"
    )


def assemble(directories, destination, expected_commit):
    require(not destination.exists(), "candidate output must not already exist")
    source = source_identity()
    require(source["commit"] == expected_commit, "assembly checkout SHA mismatch")
    workflow = workflow_identity()
    receipts = {}
    paths = {}
    for directory in directories:
        receipt_path = directory / RECEIPT
        require(
            receipt_path.is_file() and not receipt_path.is_symlink(),
            "missing platform certification",
        )
        data = receipt_path.read_bytes()
        scan(data)
        receipt = json.loads(data)
        require(data == json_bytes(receipt), "noncanonical platform record")
        platform = receipt["platform"]
        require(
            platform in TARGETS and platform not in receipts,
            "duplicate/unsupported platform",
        )
        require(
            set(receipt)
            == {
                "schema_version",
                "platform",
                "version",
                "source",
                "workflow",
                "runtime",
                "toolchain",
                "artifacts",
            }
            and receipt["schema_version"] == 1
            and receipt["version"] == VERSION
            and receipt["source"] == source
            and receipt["workflow"] == workflow
            and receipt["toolchain"] == {"rustc": "1.85.0", "cargo": "1.85.0"},
            "platform provenance mismatch",
        )
        require(
            directory_names(directory)
            == {cli_name(platform), wheel_cert.expected_filename(platform), RECEIPT},
            "unexpected platform files",
        )
        expected = [
            artifact_entry(directory / cli_name(platform), "cli", platform),
            artifact_entry(
                directory / wheel_cert.expected_filename(platform), "wheel", platform
            ),
        ]
        require(
            receipt["artifacts"] == expected,
            "artifact differs from smoke-tested platform record",
        )
        receipts[platform] = receipt
        paths[platform] = directory
    require(set(receipts) == set(TARGETS), "both mandatory platforms are required")
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="datapack-bundle-") as temporary:
        staging = Path(temporary)
        for platform, directory in paths.items():
            for name in (cli_name(platform), wheel_cert.expected_filename(platform)):
                shutil.copyfile(directory / name, staging / name)
        (staging / NOTES).write_bytes(documents()["RELEASE-NOTES.md"])
        (staging / MANIFEST).write_bytes(
            json_bytes(manifest_data(staging, receipts, source, workflow))
        )
        (staging / SUMS).write_bytes(checksum_bytes(staging))
        verify_bundle(staging, expected_commit)
        shutil.copytree(staging, destination)
    verify_bundle(destination, expected_commit)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("configure-wheel", help="set hosted wheel path remapping")
    build = commands.add_parser("build", help="build and certify one native platform")
    build.add_argument("--platform", choices=TARGETS, required=True)
    build.add_argument("--out", type=Path, required=True)
    build.add_argument(
        "--wheel-dir",
        type=Path,
        help="reuse the wheel built by the shared hosted action",
    )
    build.add_argument(
        "--allow-dirty",
        action="store_true",
        help="local rehearsal only; assembly rejects dirty sources",
    )
    bundle = commands.add_parser(
        "assemble", help="require both certified platform outputs"
    )
    bundle.add_argument("--platform-dir", action="append", type=Path, required=True)
    bundle.add_argument("--out", type=Path, required=True)
    bundle.add_argument("--expected-commit", required=True)
    verify = commands.add_parser(
        "verify", help="verify with the matching clean checkout"
    )
    verify.add_argument("--bundle", type=Path, required=True)
    verify.add_argument("--expected-commit", required=True)
    args = parser.parse_args()
    if args.command == "configure-wheel":
        configure_wheel()
    elif args.command == "build":
        build_platform(
            args.out.resolve(), args.platform, args.wheel_dir, args.allow_dirty
        )
    elif args.command == "assemble":
        assemble(args.platform_dir, args.out.resolve(), args.expected_commit)
    else:
        verify_bundle(args.bundle, args.expected_commit)


if __name__ == "__main__":
    try:
        main()
    except (
        OSError,
        ValueError,
        KeyError,
        TypeError,
        subprocess.SubprocessError,
        tarfile.TarError,
        zipfile.BadZipFile,
    ) as error:
        print(f"Release candidate: FAIL: {error}", file=sys.stderr)
        raise SystemExit(1) from error
