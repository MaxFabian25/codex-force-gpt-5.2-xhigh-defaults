from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from run_artifact_runtime_clean_host_smoke import AUTH_FILENAMES
from run_artifact_runtime_clean_host_smoke import SMOKE_SUBTITLE
from run_artifact_runtime_clean_host_smoke import SMOKE_TITLE
from run_artifact_runtime_clean_host_smoke import build_smoke_prompt
from run_artifact_runtime_clean_host_smoke import copy_auth_materials
from run_artifact_runtime_clean_host_smoke import detect_runtime_platform
from run_artifact_runtime_clean_host_smoke import normalize_release_root
from run_artifact_runtime_clean_host_smoke import write_smoke_config


class RunArtifactRuntimeCleanHostSmokeTests(unittest.TestCase):
    def test_detect_runtime_platform_linux_x64(self) -> None:
        self.assertEqual(
            detect_runtime_platform(system_name="Linux", machine_name="x86_64"),
            "linux-x64",
        )

    def test_detect_runtime_platform_darwin_arm64(self) -> None:
        self.assertEqual(
            detect_runtime_platform(system_name="Darwin", machine_name="arm64"),
            "darwin-arm64",
        )

    def test_write_smoke_config_enables_artifact(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            codex_home = Path(temp_dir)
            config_path = write_smoke_config(codex_home, model="gpt-5.4")
            contents = config_path.read_text(encoding="utf-8")
            self.assertIn('model = "gpt-5.4"', contents)
            self.assertIn("[features]", contents)
            self.assertIn("artifact = true", contents)

    def test_copy_auth_materials_copies_supported_files(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            source_home = Path(temp_dir) / "source"
            dest_home = Path(temp_dir) / "dest"
            source_home.mkdir()
            dest_home.mkdir()
            for file_name in AUTH_FILENAMES:
                (source_home / file_name).write_text("{}", encoding="utf-8")

            copied = copy_auth_materials(source_home, dest_home)

            self.assertEqual(
                {path.name for path in copied},
                set(AUTH_FILENAMES),
            )
            for file_name in AUTH_FILENAMES:
                self.assertTrue((dest_home / file_name).is_file())

    def test_build_smoke_prompt_contains_expected_artifact_contract(self) -> None:
        prompt = build_smoke_prompt(Path("/tmp/smoke.pptx"))
        self.assertIn("Use the artifacts tool exactly once.", prompt)
        self.assertIn('slide.setLayout("title slide");', prompt)
        self.assertIn(json.dumps(SMOKE_TITLE), prompt)
        self.assertIn(json.dumps(SMOKE_SUBTITLE), prompt)
        self.assertIn("reply with only the absolute path", prompt)

    def test_normalize_release_root_accepts_flat_assets_directory(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_root = Path(temp_dir)
            release_root = temp_root / "flat"
            destination_root = temp_root / "normalized"
            release_root.mkdir()
            release_tag = "artifact-runtime-v2.5.6"
            manifest_name = f"{release_tag}-manifest.json"
            (release_root / manifest_name).write_text("{}", encoding="utf-8")
            archive_name = f"{release_tag}-linux-x64.zip"
            (release_root / archive_name).write_bytes(b"zip")

            normalized_root = normalize_release_root(
                release_root,
                runtime_version="2.5.6",
                destination_root=destination_root,
            )

            self.assertEqual(normalized_root, destination_root)
            self.assertTrue(
                (destination_root / release_tag / manifest_name).is_file()
            )
            self.assertTrue(
                (destination_root / release_tag / archive_name).is_file()
            )


if __name__ == "__main__":
    unittest.main()
