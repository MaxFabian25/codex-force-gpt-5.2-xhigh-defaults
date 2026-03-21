#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from urllib import error, request
from urllib.parse import urljoin

DEFAULT_RELEASE_BASE_URL = "https://github.com/openai/codex/releases/download/"
DEFAULT_REQUIRED_PLATFORMS = (
    "darwin-arm64",
    "darwin-x64",
    "linux-arm64",
    "linux-x64",
    "windows-arm64",
    "windows-x64",
)
ARTIFACT_RUNTIME_TAG_PREFIX = "artifact-runtime-v"


class VerificationError(RuntimeError):
    pass


@dataclass(frozen=True)
class VerificationResult:
    runtime_version: str
    release_tag: str
    manifest_url: str
    archive_urls: tuple[str, ...]


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Verify that the pinned artifact runtime release exists and is complete."
    )
    parser.add_argument(
        "--versions-file",
        default="codex-rs/core/src/packages/versions.rs",
        help="Path to versions.rs containing the pinned ARTIFACT_RUNTIME constant.",
    )
    parser.add_argument(
        "--base-url",
        default=DEFAULT_RELEASE_BASE_URL,
        help="Base download URL for artifact runtime releases.",
    )
    parser.add_argument(
        "--required-platform",
        action="append",
        dest="required_platforms",
        help="Override the required platform set. Repeat to require multiple platforms.",
    )
    return parser.parse_args(argv)


def extract_runtime_version(versions_file: Path) -> str:
    contents = versions_file.read_text(encoding="utf-8")
    match = re.search(
        r'ARTIFACT_RUNTIME:\s*&str\s*=\s*"(?P<version>[^"]+)"',
        contents,
    )
    if not match:
        raise VerificationError(
            f"failed to find ARTIFACT_RUNTIME in {versions_file}"
        )
    return match.group("version")


def build_release_tag(runtime_version: str) -> str:
    return f"{ARTIFACT_RUNTIME_TAG_PREFIX}{runtime_version}"


def build_asset_url(base_url: str, release_tag: str, asset_name: str) -> str:
    normalized_base = base_url.rstrip("/") + "/"
    return urljoin(normalized_base, f"{release_tag}/{asset_name}")


def fetch_json_url(url: str) -> dict:
    req = request.Request(
        url,
        headers={
            "Accept": "application/json",
            "User-Agent": "codex-artifact-runtime-verifier",
        },
    )
    try:
        with request.urlopen(req) as response:
            return json.load(response)
    except error.HTTPError as exc:
        raise VerificationError(f"failed to fetch manifest {url}: HTTP {exc.code}") from exc
    except error.URLError as exc:
        raise VerificationError(f"failed to fetch manifest {url}: {exc.reason}") from exc


def url_exists(url: str) -> bool:
    req = request.Request(
        url,
        method="HEAD",
        headers={"User-Agent": "codex-artifact-runtime-verifier"},
    )
    try:
        with request.urlopen(req):
            return True
    except error.HTTPError as exc:
        if exc.code == 405:
            get_req = request.Request(
                url,
                headers={"User-Agent": "codex-artifact-runtime-verifier"},
            )
            try:
                with request.urlopen(get_req):
                    return True
            except error.HTTPError as inner_exc:
                if inner_exc.code == 404:
                    return False
                raise VerificationError(
                    f"failed to verify archive {url}: HTTP {inner_exc.code}"
                ) from inner_exc
            except error.URLError as inner_exc:
                raise VerificationError(
                    f"failed to verify archive {url}: {inner_exc.reason}"
                ) from inner_exc
        if exc.code == 404:
            return False
        raise VerificationError(f"failed to verify archive {url}: HTTP {exc.code}") from exc
    except error.URLError as exc:
        raise VerificationError(f"failed to verify archive {url}: {exc.reason}") from exc


def verify_release(
    runtime_version: str,
    *,
    base_url: str = DEFAULT_RELEASE_BASE_URL,
    required_platforms: tuple[str, ...] = DEFAULT_REQUIRED_PLATFORMS,
    fetch_json=fetch_json_url,
    archive_exists=url_exists,
) -> VerificationResult:
    release_tag = build_release_tag(runtime_version)
    manifest_name = f"{release_tag}-manifest.json"
    manifest_url = build_asset_url(base_url, release_tag, manifest_name)
    manifest = fetch_json(manifest_url)

    manifest_runtime_version = manifest.get("runtime_version")
    if manifest_runtime_version != runtime_version:
        raise VerificationError(
            "artifact runtime manifest version mismatch: "
            f"expected {runtime_version}, got {manifest_runtime_version!r}"
        )

    manifest_release_tag = manifest.get("release_tag")
    if manifest_release_tag != release_tag:
        raise VerificationError(
            "artifact runtime release_tag mismatch: "
            f"expected {release_tag}, got {manifest_release_tag!r}"
        )

    platforms = manifest.get("platforms")
    if not isinstance(platforms, dict):
        raise VerificationError("artifact runtime manifest is missing the `platforms` map")

    missing_platforms = [
        platform for platform in required_platforms if platform not in platforms
    ]
    if missing_platforms:
        raise VerificationError(
            "artifact runtime manifest is missing required platforms: "
            + ", ".join(missing_platforms)
        )

    archive_urls: list[str] = []
    for platform in required_platforms:
        archive_info = platforms[platform]
        archive_name = archive_info.get("archive") if isinstance(archive_info, dict) else None
        if not archive_name:
            raise VerificationError(
                f"artifact runtime manifest entry for {platform} is missing `archive`"
            )
        archive_url = build_asset_url(base_url, release_tag, archive_name)
        if not archive_exists(archive_url):
            raise VerificationError(
                f"artifact runtime archive for {platform} is missing: {archive_url}"
            )
        archive_urls.append(archive_url)

    return VerificationResult(
        runtime_version=runtime_version,
        release_tag=release_tag,
        manifest_url=manifest_url,
        archive_urls=tuple(archive_urls),
    )


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    required_platforms = tuple(args.required_platforms or DEFAULT_REQUIRED_PLATFORMS)
    runtime_version = extract_runtime_version(Path(args.versions_file))
    try:
        result = verify_release(
            runtime_version,
            base_url=args.base_url,
            required_platforms=required_platforms,
        )
    except VerificationError as exc:
        print(f"artifact runtime verification failed: {exc}", file=sys.stderr)
        return 1

    print(
        "verified artifact runtime release "
        f"{result.release_tag} ({result.runtime_version})"
    )
    print(f"manifest: {result.manifest_url}")
    for archive_url in result.archive_urls:
        print(f"archive: {archive_url}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
