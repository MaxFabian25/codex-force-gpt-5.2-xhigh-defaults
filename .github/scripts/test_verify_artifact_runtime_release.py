from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

import verify_artifact_runtime_release as verifier


class VerifyArtifactRuntimeReleaseTests(unittest.TestCase):
    def test_extract_runtime_version_from_versions_rs(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            versions_file = Path(temp_dir) / "versions.rs"
            versions_file.write_text(
                'pub(crate) const ARTIFACT_RUNTIME: &str = "2.5.6";\n',
                encoding="utf-8",
            )

            self.assertEqual(verifier.extract_runtime_version(versions_file), "2.5.6")

    def test_verify_release_accepts_complete_manifest(self) -> None:
        manifest = {
            "runtime_version": "2.5.6",
            "release_tag": "artifact-runtime-v2.5.6",
            "platforms": {
                platform: {"archive": f"artifact-runtime-v2.5.6-{platform}.zip"}
                for platform in verifier.DEFAULT_REQUIRED_PLATFORMS
            },
        }
        seen_urls: list[str] = []

        def fetch_json(url: str) -> dict:
            seen_urls.append(url)
            return manifest

        def archive_exists(url: str) -> bool:
            seen_urls.append(url)
            return True

        result = verifier.verify_release(
            "2.5.6",
            base_url="https://example.test/releases/",
            fetch_json=fetch_json,
            archive_exists=archive_exists,
        )

        self.assertEqual(result.release_tag, "artifact-runtime-v2.5.6")
        self.assertEqual(len(result.archive_urls), len(verifier.DEFAULT_REQUIRED_PLATFORMS))
        self.assertTrue(
            any(url.endswith("artifact-runtime-v2.5.6-manifest.json") for url in seen_urls)
        )

    def test_verify_release_rejects_runtime_version_mismatch(self) -> None:
        manifest = {
            "runtime_version": "2.4.0",
            "release_tag": "artifact-runtime-v2.5.6",
            "platforms": {
                "linux-x64": {"archive": "artifact-runtime-v2.5.6-linux-x64.zip"},
            },
        }

        with self.assertRaisesRegex(
            verifier.VerificationError, "manifest version mismatch"
        ):
            verifier.verify_release(
                "2.5.6",
                required_platforms=("linux-x64",),
                fetch_json=lambda _url: manifest,
                archive_exists=lambda _url: True,
            )

    def test_verify_release_rejects_missing_required_platform(self) -> None:
        manifest = {
            "runtime_version": "2.5.6",
            "release_tag": "artifact-runtime-v2.5.6",
            "platforms": {
                "linux-x64": {"archive": "artifact-runtime-v2.5.6-linux-x64.zip"},
            },
        }

        with self.assertRaisesRegex(
            verifier.VerificationError, "missing required platforms: windows-x64"
        ):
            verifier.verify_release(
                "2.5.6",
                required_platforms=("linux-x64", "windows-x64"),
                fetch_json=lambda _url: manifest,
                archive_exists=lambda _url: True,
            )


if __name__ == "__main__":
    unittest.main()
