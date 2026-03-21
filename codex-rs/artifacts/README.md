# codex-artifacts

Runtime and process-management helpers for Codex artifact generation.

This crate has two main responsibilities:

- locating, validating, and optionally downloading the pinned artifact runtime
- spawning the artifact build or render command against that runtime

## Module layout

- `src/client.rs`
  Runs build and render commands once a runtime has been resolved.
- `src/runtime/manager.rs`
  Defines the release locator and the package-manager-backed runtime installer.
- `src/runtime/installed.rs`
  Loads an extracted runtime from disk and validates its manifest and entrypoints.
- `src/runtime/js_runtime.rs`
  Chooses the JavaScript executable to use for artifact execution.
- `src/runtime/manifest.rs`
  Manifest types for release metadata and extracted runtimes.
- `src/runtime/error.rs`
  Public runtime-loading and installation errors.
- `src/tests.rs`
  Crate-level tests that exercise the public API and integration seams.

## Public API

- `ArtifactRuntimeManager`
  Resolves or installs a runtime package into `~/.codex/packages/artifacts/...`.
- `load_cached_runtime`
  Reads a previously installed runtime from a caller-provided cache root without attempting a download.
- `is_js_runtime_available`
  Checks whether artifact execution is possible with either a cached runtime or a host JS runtime.
- `ArtifactsClient`
  Executes artifact build or render requests using either a managed or preinstalled runtime.

## Runtime overrides

The CLI-side artifacts handler supports operator overrides through environment
variables:

- `CODEX_ARTIFACT_RUNTIME_ROOT`
  Load a preinstalled runtime package directly from this root directory.
- `CODEX_ARTIFACT_RUNTIME_CACHE_ROOT`
  Override the managed runtime cache root.
- `CODEX_ARTIFACT_RUNTIME_RELEASE_BASE_URL`
  Override the release download base URL.
- `CODEX_ARTIFACT_RUNTIME_VERSION`
  Override the pinned runtime version.

These are intended for local validation and release recovery. Normal production
releases should continue to rely on the pinned default version and the public
release verifier.

## Release packaging

The runtime package itself is not built from this repository. Instead, runtime
release assets are staged from prebuilt per-platform payloads that already
contain an extracted `@oai/artifact-tool` package root.

- `.github/scripts/stage_artifact_runtime_release.py`
  Validates those payloads, writes platform archives, and emits the
  `artifact-runtime-v<version>-manifest.json` file expected by the package
  manager.
- `.github/workflows/artifact-runtime-release.yml`
  Downloads per-platform payload artifacts named `artifact-runtime-<platform>`
  from a source GitHub Actions run, stages the release assets, and can publish
  or update the matching `artifact-runtime-v<version>` GitHub release tag.
- `.github/scripts/run_artifact_runtime_clean_host_smoke.py`
  Creates a temporary `CODEX_HOME`, serves either a staged runtime release or a
  one-platform package-derived release over local HTTP, runs `codex exec`
  without `CODEX_ARTIFACT_RUNTIME_ROOT`, and verifies both the managed install
  cache path and the exported `.pptx` contents.
- `.github/workflows/artifact-runtime-clean-host-smoke.yml`
  Downloads a Linux codex build artifact plus `artifact-runtime-release-assets`
  from prior workflow runs and executes the clean-host smoke against them.

## Presentation fallbacks

When the packaged runtime does not include optional presentation template
plugins, the JS wrapper that Codex executes installs a minimal built-in layout
registry for fresh presentations before user code runs.

- `presentation.template().layouts.getItem("blank")`
  Returns a blank layout object that clears slide placeholders.
- `presentation.template().layouts.getItem("title slide")`
  Returns a title-slide layout object backed by the runtime's default title and
  subtitle placeholders.

This keeps `slide.setLayout(...)` usable for the basic blank/title-slide cases
without requiring the full optional template plugin surface.
