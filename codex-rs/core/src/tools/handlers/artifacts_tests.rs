use super::*;
use crate::packages::versions;
use serial_test::serial;
use std::env;
use std::ffi::OsStr;
use tempfile::TempDir;

struct EnvVarGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &OsStr) -> Self {
        let original = env::var_os(key);
        unsafe {
            env::set_var(key, value);
        }
        Self { key, original }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.original.take() {
            Some(value) => unsafe {
                env::set_var(self.key, value);
            },
            None => unsafe {
                env::remove_var(self.key);
            },
        }
    }
}

#[test]
fn parse_freeform_args_without_pragma() {
    let args = parse_freeform_args("console.log('ok');").expect("parse args");
    assert_eq!(args.source, "console.log('ok');");
    assert_eq!(args.timeout_ms, None);
}

#[test]
fn parse_freeform_args_with_artifact_tool_pragma() {
    let args = parse_freeform_args("// codex-artifact-tool: timeout_ms=45000\nconsole.log('ok');")
        .expect("parse args");
    assert_eq!(args.source, "console.log('ok');");
    assert_eq!(args.timeout_ms, Some(45_000));
}

#[test]
fn parse_freeform_args_rejects_json_wrapped_code() {
    let err = parse_freeform_args("{\"code\":\"console.log('ok')\"}").expect_err("expected error");
    assert!(
        err.to_string()
            .contains("artifacts is a freeform tool and expects raw JavaScript source")
    );
}

#[test]
fn default_runtime_manager_uses_openai_codex_release_base() {
    let codex_home = TempDir::new().expect("create temp codex home");
    let manager = default_runtime_manager(codex_home.path().to_path_buf());

    assert_eq!(
        manager.config().release().base_url().as_str(),
        "https://github.com/openai/codex/releases/download/"
    );
    assert_eq!(
        manager.config().release().runtime_version(),
        versions::ARTIFACT_RUNTIME
    );
}

#[test]
#[serial(artifact_runtime_env)]
fn runtime_manager_from_env_applies_release_and_cache_overrides() {
    let codex_home = TempDir::new().expect("create temp codex home");
    let cache_root = codex_home.path().join("custom-cache");
    let _base_url_guard = EnvVarGuard::set(
        ARTIFACT_RUNTIME_RELEASE_BASE_URL_ENV,
        OsStr::new("https://example.test/releases/"),
    );
    let _version_guard = EnvVarGuard::set(ARTIFACT_RUNTIME_VERSION_ENV, OsStr::new("9.9.9"));
    let _cache_guard = EnvVarGuard::set(ARTIFACT_RUNTIME_CACHE_ROOT_ENV, cache_root.as_os_str());

    let manager =
        runtime_manager_from_env(codex_home.path().to_path_buf()).expect("manager from env");

    assert_eq!(
        manager.config().release().base_url().as_str(),
        "https://example.test/releases/"
    );
    assert_eq!(manager.config().release().runtime_version(), "9.9.9");
    assert_eq!(manager.config().cache_root(), cache_root);
}

#[test]
#[serial(artifact_runtime_env)]
fn runtime_manager_from_env_rejects_invalid_release_base_url_override() {
    let codex_home = TempDir::new().expect("create temp codex home");
    let _base_url_guard = EnvVarGuard::set(
        ARTIFACT_RUNTIME_RELEASE_BASE_URL_ENV,
        OsStr::new("not-a-valid-url"),
    );

    let error =
        runtime_manager_from_env(codex_home.path().to_path_buf()).expect_err("invalid base url");

    assert!(
        error
            .to_string()
            .contains(ARTIFACT_RUNTIME_RELEASE_BASE_URL_ENV)
    );
}

#[test]
#[serial(artifact_runtime_env)]
fn load_installed_runtime_from_env_reads_runtime_root_override() {
    let codex_home = TempDir::new().expect("create temp codex home");
    let platform =
        codex_artifacts::ArtifactRuntimePlatform::detect_current().expect("detect platform");
    let install_dir = codex_home
        .path()
        .join("runtime-root")
        .join(platform.as_str());
    write_runtime_package(&install_dir, versions::ARTIFACT_RUNTIME);
    let _runtime_root_guard = EnvVarGuard::set(ARTIFACT_RUNTIME_ROOT_ENV, install_dir.as_os_str());
    let _bad_base_url_guard = EnvVarGuard::set(
        ARTIFACT_RUNTIME_RELEASE_BASE_URL_ENV,
        OsStr::new("not-a-valid-url"),
    );

    let runtime = load_installed_runtime_from_env()
        .expect("runtime root override")
        .expect("installed runtime");

    assert_eq!(runtime.runtime_version(), versions::ARTIFACT_RUNTIME);
    assert_eq!(runtime.platform(), platform);
    assert_eq!(
        runtime.build_js_path(),
        install_dir.join("dist/artifact_tool.mjs")
    );
}

#[test]
fn load_cached_runtime_reads_pinned_cache_path() {
    let codex_home = TempDir::new().expect("create temp codex home");
    let platform =
        codex_artifacts::ArtifactRuntimePlatform::detect_current().expect("detect platform");
    let install_dir = codex_home
        .path()
        .join("packages")
        .join("artifacts")
        .join(versions::ARTIFACT_RUNTIME)
        .join(platform.as_str());
    write_runtime_package(&install_dir, versions::ARTIFACT_RUNTIME);

    let runtime = codex_artifacts::load_cached_runtime(
        &codex_home
            .path()
            .join(codex_artifacts::DEFAULT_CACHE_ROOT_RELATIVE),
        versions::ARTIFACT_RUNTIME,
    )
    .expect("resolve runtime");
    assert_eq!(runtime.runtime_version(), versions::ARTIFACT_RUNTIME);
    assert_eq!(
        runtime.build_js_path(),
        install_dir.join("dist/artifact_tool.mjs")
    );
}

#[test]
fn format_artifact_output_includes_success_message_when_silent() {
    let formatted = format_artifact_output(&ArtifactCommandOutput {
        exit_code: Some(0),
        stdout: String::new(),
        stderr: String::new(),
    });
    assert!(formatted.contains("artifact JS completed successfully."));
}

fn write_runtime_package(install_dir: &std::path::Path, runtime_version: &str) {
    std::fs::create_dir_all(install_dir.join("dist")).expect("create build entrypoint dir");
    std::fs::write(
        install_dir.join("package.json"),
        serde_json::json!({
            "name": "@oai/artifact-tool",
            "version": runtime_version,
            "type": "module",
            "exports": {
                ".": "./dist/artifact_tool.mjs"
            }
        })
        .to_string(),
    )
    .expect("write package json");
    std::fs::write(
        install_dir.join("dist/artifact_tool.mjs"),
        "export const ok = true;\n",
    )
    .expect("write build entrypoint");
}
