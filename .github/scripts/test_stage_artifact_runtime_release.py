from __future__ import annotations

import json
import tempfile
import unittest
import zipfile
from pathlib import Path

import stage_artifact_runtime_release as stager


def write_runtime_package(root_dir: Path, runtime_version: str) -> None:
    (root_dir / "dist").mkdir(parents=True, exist_ok=True)
    (root_dir / "package.json").write_text(
        json.dumps(
            {
                "name": "@oai/artifact-tool",
                "version": runtime_version,
                "type": "module",
                "exports": {".": "./dist/artifact_tool.mjs"},
            }
        ),
        encoding="utf-8",
    )
    (root_dir / "dist" / "artifact_tool.mjs").write_text(
        "export const ok = true;\n",
        encoding="utf-8",
    )


class StageArtifactRuntimeReleaseTests(unittest.TestCase):
    def test_extract_runtime_version_from_versions_rs(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            versions_file = Path(temp_dir) / "versions.rs"
            versions_file.write_text(
                'pub(crate) const ARTIFACT_RUNTIME: &str = "2.5.6";\n',
                encoding="utf-8",
            )

            self.assertEqual(stager.extract_runtime_version(versions_file), "2.5.6")

    def test_stage_release_writes_manifest_and_archives(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_root = Path(temp_dir)
            linux_root = temp_root / "linux-x64" / "package"
            windows_root = temp_root / "windows-x64" / "package"
            write_runtime_package(linux_root, "2.5.6")
            write_runtime_package(windows_root, "2.5.6")

            result = stager.stage_release(
                "2.5.6",
                output_dir=temp_root / "dist",
                platform_roots={
                    "linux-x64": temp_root / "linux-x64",
                    "windows-x64": temp_root / "windows-x64",
                },
                required_platforms=("linux-x64", "windows-x64"),
                node_version="22.14.0",
            )

            manifest = json.loads(result.manifest_path.read_text(encoding="utf-8"))
            self.assertEqual(result.release_tag, "artifact-runtime-v2.5.6")
            self.assertEqual(manifest["node_version"], "22.14.0")
            self.assertEqual(
                set(manifest["platforms"]),
                {"linux-x64", "windows-x64"},
            )
            archive_path = temp_root / "dist" / manifest["platforms"]["linux-x64"]["archive"]
            self.assertTrue(archive_path.is_file())
            with zipfile.ZipFile(archive_path) as archive:
                self.assertIn("artifact-runtime/package.json", archive.namelist())
                self.assertIn(
                    "artifact-runtime/dist/artifact_tool.mjs",
                    archive.namelist(),
                )

    def test_stage_release_rejects_missing_required_platform(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_root = Path(temp_dir)
            linux_root = temp_root / "linux-x64"
            write_runtime_package(linux_root, "2.5.6")

            with self.assertRaisesRegex(
                stager.StageError,
                "missing runtime roots for required platforms: windows-x64",
            ):
                stager.stage_release(
                    "2.5.6",
                    output_dir=temp_root / "dist",
                    platform_roots={"linux-x64": linux_root},
                    required_platforms=("linux-x64", "windows-x64"),
                )

    def test_stage_release_rejects_runtime_version_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_root = Path(temp_dir)
            linux_root = temp_root / "linux-x64"
            write_runtime_package(linux_root, "2.4.0")

            with self.assertRaisesRegex(
                stager.StageError,
                "artifact runtime version mismatch",
            ):
                stager.stage_release(
                    "2.5.6",
                    output_dir=temp_root / "dist",
                    platform_roots={"linux-x64": linux_root},
                    required_platforms=("linux-x64",),
                )


if __name__ == "__main__":
    unittest.main()
