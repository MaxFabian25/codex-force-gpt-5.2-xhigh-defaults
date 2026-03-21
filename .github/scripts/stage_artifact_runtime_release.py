#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import re
import stat
import sys
import zipfile
from dataclasses import dataclass
from pathlib import Path

DEFAULT_REQUIRED_PLATFORMS = (
    "darwin-arm64",
    "darwin-x64",
    "linux-arm64",
    "linux-x64",
    "windows-arm64",
    "windows-x64",
)
EXPECTED_PACKAGE_NAME = "@oai/artifact-tool"
RELEASE_TAG_PREFIX = "artifact-runtime-v"


class StageError(RuntimeError):
    pass


@dataclass(frozen=True)
class RuntimePackage:
    root_dir: Path
    build_entrypoint: Path


@dataclass(frozen=True)
class StageResult:
    runtime_version: str
    release_tag: str
    manifest_path: Path
    archive_paths: tuple[Path, ...]


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Stage GitHub release assets for the packaged artifact runtime."
    )
    parser.add_argument(
        "--versions-file",
        default="codex-rs/core/src/packages/versions.rs",
        help="Path to versions.rs containing the pinned ARTIFACT_RUNTIME constant.",
    )
    parser.add_argument(
        "--runtime-version",
        default=None,
        help="Override the runtime version instead of reading versions.rs.",
    )
    parser.add_argument(
        "--output-dir",
        required=True,
        help="Directory where the manifest and archives should be written.",
    )
    parser.add_argument(
        "--runtime-root",
        action="append",
        default=[],
        metavar="PLATFORM=PATH",
        help=(
            "Path to an extracted runtime package or a directory containing a single "
            "runtime package for a platform. Repeat once per platform."
        ),
    )
    parser.add_argument(
        "--required-platform",
        action="append",
        dest="required_platforms",
        help="Override the required platform set. Repeat to require multiple platforms.",
    )
    parser.add_argument(
        "--node-version",
        default=None,
        help="Optional Node.js version string to record in the release manifest.",
    )
    return parser.parse_args(argv)


def extract_runtime_version(versions_file: Path) -> str:
    contents = versions_file.read_text(encoding="utf-8")
    match = re.search(
        r'ARTIFACT_RUNTIME:\s*&str\s*=\s*"(?P<version>[^"]+)"',
        contents,
    )
    if not match:
        raise StageError(f"failed to find ARTIFACT_RUNTIME in {versions_file}")
    return match.group("version")


def build_release_tag(runtime_version: str) -> str:
    return f"{RELEASE_TAG_PREFIX}{runtime_version}"


def parse_platform_root(value: str) -> tuple[str, Path]:
    platform, separator, raw_path = value.partition("=")
    platform = platform.strip()
    raw_path = raw_path.strip()
    if not separator or not platform or not raw_path:
        raise StageError(
            f"invalid --runtime-root value {value!r}; expected PLATFORM=PATH"
        )
    return platform, Path(raw_path)


def stage_release(
    runtime_version: str,
    *,
    output_dir: Path,
    platform_roots: dict[str, Path],
    required_platforms: tuple[str, ...] = DEFAULT_REQUIRED_PLATFORMS,
    node_version: str | None = None,
) -> StageResult:
    missing_platforms = [
        platform for platform in required_platforms if platform not in platform_roots
    ]
    if missing_platforms:
        raise StageError(
            "missing runtime roots for required platforms: "
            + ", ".join(missing_platforms)
        )

    output_dir.mkdir(parents=True, exist_ok=True)
    release_tag = build_release_tag(runtime_version)
    archive_paths: list[Path] = []
    manifest_platforms: dict[str, dict[str, object]] = {}

    for platform in required_platforms:
        package = load_runtime_package(platform_roots[platform], runtime_version)
        archive_name = f"{release_tag}-{platform}.zip"
        archive_path = output_dir / archive_name
        write_runtime_archive(package.root_dir, archive_path)
        archive_bytes = archive_path.read_bytes()
        archive_paths.append(archive_path)
        manifest_platforms[platform] = {
            "archive": archive_name,
            "sha256": hashlib.sha256(archive_bytes).hexdigest(),
            "format": "zip",
            "size_bytes": len(archive_bytes),
        }

    manifest = {
        "schema_version": 1,
        "runtime_version": runtime_version,
        "release_tag": release_tag,
        "node_version": node_version,
        "platforms": manifest_platforms,
    }
    manifest_path = output_dir / f"{release_tag}-manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")

    return StageResult(
        runtime_version=runtime_version,
        release_tag=release_tag,
        manifest_path=manifest_path,
        archive_paths=tuple(archive_paths),
    )


def load_runtime_package(runtime_root_hint: Path, runtime_version: str) -> RuntimePackage:
    root_dir = detect_runtime_root(runtime_root_hint)
    package_json_path = root_dir / "package.json"
    try:
        package_json = json.loads(package_json_path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise StageError(f"missing runtime package metadata: {package_json_path}") from exc
    except json.JSONDecodeError as exc:
        raise StageError(f"invalid runtime package metadata: {package_json_path}") from exc

    package_name = package_json.get("name")
    if package_name != EXPECTED_PACKAGE_NAME:
        raise StageError(
            "unsupported artifact runtime package at "
            f"{package_json_path}: expected name {EXPECTED_PACKAGE_NAME!r}, "
            f"got {package_name!r}"
        )

    package_version = package_json.get("version")
    if package_version != runtime_version:
        raise StageError(
            "artifact runtime version mismatch at "
            f"{package_json_path}: expected {runtime_version!r}, got {package_version!r}"
        )

    build_entrypoint = resolve_build_entrypoint(package_json_path, package_json)
    if not build_entrypoint.is_file():
        raise StageError(f"missing artifact runtime build entrypoint: {build_entrypoint}")

    return RuntimePackage(root_dir=root_dir, build_entrypoint=build_entrypoint)


def detect_runtime_root(runtime_root_hint: Path) -> Path:
    if is_runtime_root(runtime_root_hint):
        return runtime_root_hint

    if not runtime_root_hint.is_dir():
        raise StageError(f"runtime root does not exist: {runtime_root_hint}")

    directory_candidates = sorted(
        child for child in runtime_root_hint.iterdir() if child.is_dir()
    )
    if len(directory_candidates) == 1 and is_runtime_root(directory_candidates[0]):
        return directory_candidates[0]

    raise StageError(
        f"failed to detect artifact runtime root under {runtime_root_hint}"
    )


def is_runtime_root(path: Path) -> bool:
    if not path.is_dir():
        return False
    try:
        build_entrypoint = resolve_build_entrypoint(
            path / "package.json",
            json.loads((path / "package.json").read_text(encoding="utf-8")),
        )
    except (FileNotFoundError, json.JSONDecodeError, StageError):
        return False
    return build_entrypoint.is_file()


def resolve_build_entrypoint(package_json_path: Path, package_json: dict[str, object]) -> Path:
    exports = package_json.get("exports")
    if isinstance(exports, str):
        raw_entrypoint = exports
    elif isinstance(exports, dict):
        raw_entrypoint = exports.get(".")
    else:
        raw_entrypoint = None
    if not isinstance(raw_entrypoint, str) or not raw_entrypoint.strip():
        raise StageError(
            "unsupported artifact runtime package at "
            f"{package_json_path}: expected exports['.'] to point at the JS entrypoint"
        )

    relative_path = Path(raw_entrypoint.strip())
    if relative_path.is_absolute():
        raise StageError(
            "unsupported artifact runtime package at "
            f"{package_json_path}: exports['.'] must be a relative path"
        )
    if any(part == ".." for part in relative_path.parts):
        raise StageError(
            "unsupported artifact runtime package at "
            f"{package_json_path}: exports['.'] must stay inside the runtime root"
        )
    build_entrypoint = package_json_path.parent / relative_path
    try:
        build_entrypoint.relative_to(package_json_path.parent)
    except ValueError as exc:
        raise StageError(
            "unsupported artifact runtime package at "
            f"{package_json_path}: exports['.'] must stay inside the runtime root"
        ) from exc
    return build_entrypoint


def write_runtime_archive(root_dir: Path, archive_path: Path) -> None:
    if archive_path.exists():
        archive_path.unlink()
    archive_path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(
        archive_path,
        mode="w",
        compression=zipfile.ZIP_DEFLATED,
        compresslevel=9,
    ) as archive:
        for file_path in sorted(path for path in root_dir.rglob("*") if path.is_file()):
            relative_path = file_path.relative_to(root_dir).as_posix()
            zip_info = zipfile.ZipInfo(f"artifact-runtime/{relative_path}")
            zip_info.compress_type = zipfile.ZIP_DEFLATED
            zip_info.date_time = (1980, 1, 1, 0, 0, 0)
            zip_info.external_attr = (
                (stat.S_IMODE(file_path.stat().st_mode) | stat.S_IFREG) << 16
            )
            archive.writestr(zip_info, file_path.read_bytes())


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    platform_roots = dict(parse_platform_root(value) for value in args.runtime_root)
    required_platforms = tuple(args.required_platforms or DEFAULT_REQUIRED_PLATFORMS)
    runtime_version = args.runtime_version or extract_runtime_version(
        Path(args.versions_file)
    )
    try:
        result = stage_release(
            runtime_version,
            output_dir=Path(args.output_dir),
            platform_roots=platform_roots,
            required_platforms=required_platforms,
            node_version=args.node_version,
        )
    except StageError as exc:
        print(f"artifact runtime staging failed: {exc}", file=sys.stderr)
        return 1

    print(
        f"staged artifact runtime release {result.release_tag} ({result.runtime_version})"
    )
    print(f"manifest: {result.manifest_path}")
    for archive_path in result.archive_paths:
        print(f"archive: {archive_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
