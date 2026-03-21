#!/usr/bin/env python3
from __future__ import annotations

import argparse
import contextlib
import http.server
import json
import os
import platform as host_platform
import shutil
import subprocess
import sys
import threading
import zipfile
from dataclasses import asdict
from dataclasses import dataclass
from pathlib import Path

from stage_artifact_runtime_release import StageError
from stage_artifact_runtime_release import build_release_tag
from stage_artifact_runtime_release import extract_runtime_version
from stage_artifact_runtime_release import stage_release

DEFAULT_MODEL = "gpt-5.4"
DEFAULT_SOURCE_CODEX_HOME = Path.home() / ".codex"
AUTH_FILENAMES = ("auth.json", ".credentials.json")
SMOKE_TITLE = "Artifacts Clean Host Smoke"
SMOKE_SUBTITLE = "Managed runtime install succeeded."


class SmokeError(RuntimeError):
    pass


@dataclass(frozen=True)
class SmokeResult:
    codex_binary: str
    codex_home: str
    copied_auth_files: list[str]
    runtime_version: str
    runtime_platform: str
    release_base_url: str
    installed_runtime_root: str
    output_pptx: str
    final_message_path: str
    final_message: str


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Run a clean-host artifacts smoke that proves managed runtime "
            "installation without CODEX_ARTIFACT_RUNTIME_ROOT."
        )
    )
    parser.add_argument(
        "--codex-binary",
        required=True,
        help="Path to the codex binary to run for the smoke.",
    )
    parser.add_argument(
        "--output-dir",
        required=True,
        help="Directory where temporary assets and the exported PPTX should be written.",
    )
    parser.add_argument(
        "--versions-file",
        default="codex-rs/core/src/packages/versions.rs",
        help="Path to versions.rs containing ARTIFACT_RUNTIME.",
    )
    parser.add_argument(
        "--runtime-version",
        default=None,
        help="Override the runtime version instead of reading versions.rs.",
    )
    parser.add_argument(
        "--runtime-platform",
        default=None,
        help="Platform key to smoke (defaults to the current host platform).",
    )
    parser.add_argument(
        "--runtime-package-root",
        default=None,
        help=(
            "Path to an extracted runtime package for the current platform. "
            "The script stages a one-platform release from it before running codex."
        ),
    )
    parser.add_argument(
        "--runtime-release-root",
        default=None,
        help=(
            "Path to a staged release root or a flat directory containing "
            "artifact-runtime-v<version>-manifest.json plus platform archives."
        ),
    )
    parser.add_argument(
        "--source-codex-home",
        default=str(DEFAULT_SOURCE_CODEX_HOME),
        help="Optional source CODEX_HOME to copy auth.json/.credentials.json from.",
    )
    parser.add_argument(
        "--model",
        default=DEFAULT_MODEL,
        help="Model passed to `codex exec -m ...`.",
    )
    parser.add_argument(
        "--summary-path",
        default=None,
        help="Optional JSON file path where the smoke summary should be written.",
    )
    args = parser.parse_args(argv)
    if bool(args.runtime_package_root) == bool(args.runtime_release_root):
        parser.error(
            "specify exactly one of --runtime-package-root or --runtime-release-root"
        )
    return args


def detect_runtime_platform(
    system_name: str | None = None,
    machine_name: str | None = None,
) -> str:
    system = (system_name or host_platform.system()).lower()
    machine = (machine_name or host_platform.machine()).lower()

    if machine in {"x86_64", "amd64"}:
        arch = "x64"
    elif machine in {"aarch64", "arm64"}:
        arch = "arm64"
    else:
        raise SmokeError(f"unsupported CPU architecture for artifacts smoke: {machine}")

    if system == "linux":
        os_name = "linux"
    elif system == "darwin":
        os_name = "darwin"
    elif system in {"windows", "msys", "cygwin"}:
        os_name = "windows"
    else:
        raise SmokeError(f"unsupported OS for artifacts smoke: {system}")

    return f"{os_name}-{arch}"


def write_smoke_config(codex_home: Path, *, model: str | None) -> Path:
    config_path = codex_home / "config.toml"
    lines: list[str] = []
    if model:
        lines.append(f'model = "{model}"')
    lines.extend(
        [
            'approval_policy = "never"',
            'sandbox_mode = "danger-full-access"',
            "",
            "[features]",
            "artifact = true",
        ]
    )
    config_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return config_path


def copy_auth_materials(source_home: Path, dest_home: Path) -> list[Path]:
    copied: list[Path] = []
    if not source_home.is_dir():
        return copied
    for file_name in AUTH_FILENAMES:
        source_path = source_home / file_name
        if source_path.is_file():
            dest_path = dest_home / file_name
            shutil.copy2(source_path, dest_path)
            copied.append(dest_path)
    return copied


def build_smoke_prompt(output_path: Path) -> str:
    quoted_output_path = json.dumps(str(output_path))
    return "\n".join(
        [
            "Use the artifacts tool exactly once.",
            "Do not use shell, apply_patch, or any other tool.",
            "",
            "Pass the artifacts tool raw JavaScript that does exactly this:",
            "const presentation = await Presentation.create();",
            "const slide = presentation.slides.add();",
            'slide.setLayout("title slide");',
            "const placeholders = slide.placeholders.getAll();",
            f"placeholders[0].text = {json.dumps(SMOKE_TITLE)};",
            f"placeholders[1].text = {json.dumps(SMOKE_SUBTITLE)};",
            "const file = await PresentationFile.exportPptx(presentation);",
            f"await file.save({quoted_output_path});",
            f"console.log({quoted_output_path});",
            "",
            "After the artifacts tool call completes, reply with only the absolute path to the exported .pptx file.",
        ]
    )


def stage_release_root_from_package(
    *,
    runtime_version: str,
    runtime_platform: str,
    runtime_package_root: Path,
    destination_root: Path,
) -> Path:
    release_dir = destination_root / build_release_tag(runtime_version)
    stage_release(
        runtime_version,
        output_dir=release_dir,
        platform_roots={runtime_platform: runtime_package_root},
        required_platforms=(runtime_platform,),
    )
    return destination_root


def normalize_release_root(
    release_root: Path,
    *,
    runtime_version: str,
    destination_root: Path,
) -> Path:
    release_tag = build_release_tag(runtime_version)
    manifest_name = f"{release_tag}-manifest.json"
    release_dir = release_root / release_tag

    if (release_dir / manifest_name).is_file():
        return release_root

    if release_root.name == release_tag and (release_root / manifest_name).is_file():
        target_dir = destination_root / release_tag
        if target_dir.exists():
            shutil.rmtree(target_dir)
        shutil.copytree(release_root, target_dir)
        return destination_root

    flat_manifest = release_root / manifest_name
    if flat_manifest.is_file():
        target_dir = destination_root / release_tag
        target_dir.mkdir(parents=True, exist_ok=True)
        for path in release_root.iterdir():
            if path.is_file() and path.name.startswith(f"{release_tag}-"):
                shutil.copy2(path, target_dir / path.name)
        return destination_root

    raise SmokeError(
        "failed to detect a staged artifact runtime release root under "
        f"{release_root}"
    )


class QuietSimpleHTTPRequestHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, fmt: str, *args: object) -> None:  # pragma: no cover - silence only
        del fmt, args


@contextlib.contextmanager
def serve_directory(directory: Path) -> tuple[threading.Thread, str]:
    handler = lambda *args, **kwargs: QuietSimpleHTTPRequestHandler(  # noqa: E731
        *args,
        directory=str(directory),
        **kwargs,
    )
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        host, port = server.server_address
        yield thread, f"http://{host}:{port}/"
    finally:
        server.shutdown()
        thread.join(timeout=10)
        server.server_close()


def resolve_installed_runtime_root(
    codex_home: Path,
    *,
    runtime_version: str,
    runtime_platform: str,
) -> Path:
    install_dir = codex_home / "packages" / "artifacts" / runtime_version / runtime_platform
    direct_root = install_dir / "package.json"
    if direct_root.is_file():
        return install_dir

    child_dirs = sorted(path for path in install_dir.iterdir() if path.is_dir()) if install_dir.is_dir() else []
    if len(child_dirs) == 1 and (child_dirs[0] / "package.json").is_file():
        return child_dirs[0]

    raise SmokeError(
        "managed runtime was not installed under "
        f"{install_dir}"
    )


def verify_pptx(output_pptx: Path) -> None:
    if not output_pptx.is_file():
        raise SmokeError(f"expected exported pptx at {output_pptx}")

    with zipfile.ZipFile(output_pptx) as archive:
        try:
            slide_xml = archive.read("ppt/slides/slide1.xml").decode("utf-8")
        except KeyError as exc:
            raise SmokeError(
                f"exported PPTX is missing ppt/slides/slide1.xml: {output_pptx}"
            ) from exc
    if SMOKE_TITLE not in slide_xml:
        raise SmokeError(
            f"slide XML did not contain expected title {SMOKE_TITLE!r}: {output_pptx}"
        )
    if SMOKE_SUBTITLE not in slide_xml:
        raise SmokeError(
            f"slide XML did not contain expected subtitle {SMOKE_SUBTITLE!r}: {output_pptx}"
        )


def run_codex_smoke(
    *,
    codex_binary: Path,
    output_dir: Path,
    runtime_version: str,
    runtime_platform: str,
    release_base_url: str,
    model: str | None,
    source_codex_home: Path,
) -> SmokeResult:
    codex_home = output_dir / "codex-home"
    workspace = output_dir / "workspace"
    codex_home.mkdir(parents=True, exist_ok=True)
    workspace.mkdir(parents=True, exist_ok=True)
    copied_auth = copy_auth_materials(source_codex_home, codex_home)
    write_smoke_config(codex_home, model=model)

    output_pptx = output_dir / "artifacts-clean-host-smoke.pptx"
    final_message_path = output_dir / "final-message.txt"
    prompt = build_smoke_prompt(output_pptx)

    env = os.environ.copy()
    env["CODEX_HOME"] = str(codex_home)
    env["CODEX_ARTIFACT_RUNTIME_RELEASE_BASE_URL"] = release_base_url
    env.pop("CODEX_ARTIFACT_RUNTIME_ROOT", None)

    cmd = [
        str(codex_binary),
        "exec",
        "--skip-git-repo-check",
        "--ephemeral",
        "--dangerously-bypass-approvals-and-sandbox",
        "-C",
        str(workspace),
        "--output-last-message",
        str(final_message_path),
    ]
    if model:
        cmd.extend(["-m", model])
    cmd.append(prompt)

    completed = subprocess.run(
        cmd,
        cwd=workspace,
        env=env,
        text=True,
        capture_output=True,
    )
    if completed.returncode != 0:
        raise SmokeError(
            "codex artifacts clean-host smoke failed with exit code "
            f"{completed.returncode}\nstdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )

    if not final_message_path.is_file():
        raise SmokeError(f"codex did not write {final_message_path}")
    final_message = final_message_path.read_text(encoding="utf-8").strip()
    if str(output_pptx) not in final_message:
        raise SmokeError(
            "final assistant message did not mention the exported path:\n"
            f"{final_message}"
        )

    installed_root = resolve_installed_runtime_root(
        codex_home,
        runtime_version=runtime_version,
        runtime_platform=runtime_platform,
    )
    verify_pptx(output_pptx)

    return SmokeResult(
        codex_binary=str(codex_binary),
        codex_home=str(codex_home),
        copied_auth_files=[str(path) for path in copied_auth],
        runtime_version=runtime_version,
        runtime_platform=runtime_platform,
        release_base_url=release_base_url,
        installed_runtime_root=str(installed_root),
        output_pptx=str(output_pptx),
        final_message_path=str(final_message_path),
        final_message=final_message,
    )


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    codex_binary = Path(args.codex_binary)
    if not codex_binary.is_file():
        print(f"clean-host smoke failed: codex binary not found: {codex_binary}", file=sys.stderr)
        return 1

    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    runtime_version = args.runtime_version or extract_runtime_version(Path(args.versions_file))
    runtime_platform = args.runtime_platform or detect_runtime_platform()

    prepared_release_root = output_dir / "runtime-release-root"
    prepared_release_root.mkdir(parents=True, exist_ok=True)
    try:
        if args.runtime_package_root:
            release_root = stage_release_root_from_package(
                runtime_version=runtime_version,
                runtime_platform=runtime_platform,
                runtime_package_root=Path(args.runtime_package_root),
                destination_root=prepared_release_root,
            )
        else:
            release_root = normalize_release_root(
                Path(args.runtime_release_root),
                runtime_version=runtime_version,
                destination_root=prepared_release_root,
            )

        with serve_directory(release_root) as (_server_thread, release_base_url):
            result = run_codex_smoke(
                codex_binary=codex_binary,
                output_dir=output_dir,
                runtime_version=runtime_version,
                runtime_platform=runtime_platform,
                release_base_url=release_base_url,
                model=args.model,
                source_codex_home=Path(args.source_codex_home),
            )
    except (SmokeError, StageError) as exc:
        print(f"clean-host smoke failed: {exc}", file=sys.stderr)
        return 1

    summary = json.dumps(asdict(result), indent=2) + "\n"
    if args.summary_path:
        Path(args.summary_path).write_text(summary, encoding="utf-8")
    print(summary, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
