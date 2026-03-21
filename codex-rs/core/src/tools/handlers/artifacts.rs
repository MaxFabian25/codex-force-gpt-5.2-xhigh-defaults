use async_trait::async_trait;
use codex_artifacts::ArtifactBuildRequest;
use codex_artifacts::ArtifactCommandOutput;
use codex_artifacts::ArtifactRuntimeManager;
use codex_artifacts::ArtifactRuntimeManagerConfig;
use codex_artifacts::ArtifactRuntimePlatform;
use codex_artifacts::ArtifactRuntimeReleaseLocator;
use codex_artifacts::ArtifactsClient;
use codex_artifacts::ArtifactsError;
use codex_artifacts::InstalledArtifactRuntime;
use serde_json::Value as JsonValue;
use std::env;
use std::time::Duration;
use std::time::Instant;
use url::Url;

use crate::codex::Session;
use crate::codex::TurnContext;
use crate::exec::ExecToolCallOutput;
use crate::exec::StreamOutput;
use crate::function_tool::FunctionCallError;
use crate::packages::versions;
use crate::protocol::ExecCommandSource;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::events::ToolEmitter;
use crate::tools::events::ToolEventCtx;
use crate::tools::events::ToolEventFailure;
use crate::tools::events::ToolEventStage;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;
use codex_features::Feature;

const ARTIFACTS_TOOL_NAME: &str = "artifacts";
const ARTIFACT_TOOL_PRAGMA_PREFIX: &str = "// codex-artifact-tool:";
const DEFAULT_EXECUTION_TIMEOUT: Duration = Duration::from_secs(30);
const ARTIFACT_RUNTIME_ROOT_ENV: &str = "CODEX_ARTIFACT_RUNTIME_ROOT";
const ARTIFACT_RUNTIME_CACHE_ROOT_ENV: &str = "CODEX_ARTIFACT_RUNTIME_CACHE_ROOT";
const ARTIFACT_RUNTIME_RELEASE_BASE_URL_ENV: &str = "CODEX_ARTIFACT_RUNTIME_RELEASE_BASE_URL";
const ARTIFACT_RUNTIME_VERSION_ENV: &str = "CODEX_ARTIFACT_RUNTIME_VERSION";

pub struct ArtifactsHandler;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArtifactsToolArgs {
    source: String,
    timeout_ms: Option<u64>,
}

#[async_trait]
impl ToolHandler for ArtifactsHandler {
    type Output = FunctionToolOutput;

    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Custom { .. })
    }

    async fn is_mutating(&self, _invocation: &ToolInvocation) -> bool {
        true
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<Self::Output, FunctionCallError> {
        let ToolInvocation {
            session,
            turn,
            payload,
            call_id,
            ..
        } = invocation;

        if !session.enabled(Feature::Artifact) {
            return Err(FunctionCallError::RespondToModel(
                "artifacts is disabled by feature flag".to_string(),
            ));
        }

        let args = match payload {
            ToolPayload::Custom { input } => parse_freeform_args(&input)?,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "artifacts expects freeform JavaScript input authored against the preloaded @oai/artifact-tool exports".to_string(),
                ));
            }
        };

        let client = artifacts_client(turn.config.codex_home.clone())?;

        let started_at = Instant::now();
        emit_exec_begin(session.as_ref(), turn.as_ref(), &call_id).await;

        let result = client
            .execute_build(ArtifactBuildRequest {
                source: args.source,
                cwd: turn.cwd.clone(),
                timeout: Some(Duration::from_millis(
                    args.timeout_ms
                        .unwrap_or(DEFAULT_EXECUTION_TIMEOUT.as_millis() as u64),
                )),
                env: Default::default(),
            })
            .await;

        let (success, output) = match result {
            Ok(output) => (output.success(), output),
            Err(error) => (false, error_output(&error)),
        };

        emit_exec_end(
            session.as_ref(),
            turn.as_ref(),
            &call_id,
            &output,
            started_at.elapsed(),
            success,
        )
        .await;

        Ok(FunctionToolOutput::from_text(
            format_artifact_output(&output),
            Some(success),
        ))
    }
}

fn parse_freeform_args(input: &str) -> Result<ArtifactsToolArgs, FunctionCallError> {
    if input.trim().is_empty() {
        return Err(FunctionCallError::RespondToModel(
            "artifacts expects raw JavaScript source text (non-empty) authored against the preloaded @oai/artifact-tool exports. Provide JS only, optionally with first-line `// codex-artifact-tool: timeout_ms=15000`."
                .to_string(),
        ));
    }

    let mut args = ArtifactsToolArgs {
        source: input.to_string(),
        timeout_ms: None,
    };

    let mut lines = input.splitn(2, '\n');
    let first_line = lines.next().unwrap_or_default();
    let rest = lines.next().unwrap_or_default();
    let trimmed = first_line.trim_start();
    let Some(pragma) = parse_pragma_prefix(trimmed) else {
        reject_json_or_quoted_source(&args.source)?;
        return Ok(args);
    };

    let mut timeout_ms = None;
    let directive = pragma.trim();
    if !directive.is_empty() {
        for token in directive.split_whitespace() {
            let (key, value) = token.split_once('=').ok_or_else(|| {
                FunctionCallError::RespondToModel(format!(
                    "artifacts pragma expects space-separated key=value pairs (supported keys: timeout_ms); got `{token}`"
                ))
            })?;
            match key {
                "timeout_ms" => {
                    if timeout_ms.is_some() {
                        return Err(FunctionCallError::RespondToModel(
                            "artifacts pragma specifies timeout_ms more than once".to_string(),
                        ));
                    }
                    let parsed = value.parse::<u64>().map_err(|_| {
                        FunctionCallError::RespondToModel(format!(
                            "artifacts pragma timeout_ms must be an integer; got `{value}`"
                        ))
                    })?;
                    timeout_ms = Some(parsed);
                }
                _ => {
                    return Err(FunctionCallError::RespondToModel(format!(
                        "artifacts pragma only supports timeout_ms; got `{key}`"
                    )));
                }
            }
        }
    }

    if rest.trim().is_empty() {
        return Err(FunctionCallError::RespondToModel(
            "artifacts pragma must be followed by JavaScript source on subsequent lines"
                .to_string(),
        ));
    }

    reject_json_or_quoted_source(rest)?;
    args.source = rest.to_string();
    args.timeout_ms = timeout_ms;
    Ok(args)
}

fn reject_json_or_quoted_source(code: &str) -> Result<(), FunctionCallError> {
    let trimmed = code.trim();
    if trimmed.starts_with("```") {
        return Err(FunctionCallError::RespondToModel(
            "artifacts expects raw JavaScript source, not markdown code fences. Resend plain JS only (optional first line `// codex-artifact-tool: ...`)."
                .to_string(),
        ));
    }
    let Ok(value) = serde_json::from_str::<JsonValue>(trimmed) else {
        return Ok(());
    };
    match value {
        JsonValue::Object(_) | JsonValue::String(_) => Err(FunctionCallError::RespondToModel(
            "artifacts is a freeform tool and expects raw JavaScript source authored against the preloaded @oai/artifact-tool exports. Resend plain JS only (optional first line `// codex-artifact-tool: ...`); do not send JSON (`{\"code\":...}`), quoted code, or markdown fences."
                .to_string(),
        )),
        _ => Ok(()),
    }
}

fn parse_pragma_prefix(line: &str) -> Option<&str> {
    line.strip_prefix(ARTIFACT_TOOL_PRAGMA_PREFIX)
}

fn artifacts_client(codex_home: std::path::PathBuf) -> Result<ArtifactsClient, FunctionCallError> {
    if let Some(runtime) = load_installed_runtime_from_env()? {
        return Ok(ArtifactsClient::from_installed_runtime(runtime));
    }

    Ok(ArtifactsClient::from_runtime_manager(
        runtime_manager_from_env(codex_home)?,
    ))
}

fn runtime_manager_from_env(
    codex_home: std::path::PathBuf,
) -> Result<ArtifactRuntimeManager, FunctionCallError> {
    let has_release_override = runtime_env_override(ARTIFACT_RUNTIME_RELEASE_BASE_URL_ENV)
        .is_some()
        || runtime_env_override(ARTIFACT_RUNTIME_VERSION_ENV).is_some()
        || runtime_path_override(ARTIFACT_RUNTIME_CACHE_ROOT_ENV).is_some();

    if !has_release_override {
        return Ok(default_runtime_manager(codex_home));
    }

    let runtime_version = runtime_env_override(ARTIFACT_RUNTIME_VERSION_ENV)
        .unwrap_or_else(|| versions::ARTIFACT_RUNTIME.to_string());
    let release_base_url = match runtime_env_override(ARTIFACT_RUNTIME_RELEASE_BASE_URL_ENV) {
        Some(value) => Url::parse(&value).map_err(|error| {
            FunctionCallError::RespondToModel(format!(
                "{ARTIFACT_RUNTIME_RELEASE_BASE_URL_ENV} must be a valid absolute URL; got `{value}` ({error})"
            ))
        })?,
        None => Url::parse(codex_artifacts::DEFAULT_RELEASE_BASE_URL)
            .unwrap_or_else(|error| panic!("default artifact runtime release URL must parse: {error}")),
    };
    let mut config = ArtifactRuntimeManagerConfig::new(
        codex_home,
        ArtifactRuntimeReleaseLocator::new(release_base_url, runtime_version),
    );
    if let Some(cache_root) = runtime_path_override(ARTIFACT_RUNTIME_CACHE_ROOT_ENV) {
        config = config.with_cache_root(cache_root);
    }
    Ok(ArtifactRuntimeManager::new(config))
}

fn default_runtime_manager(codex_home: std::path::PathBuf) -> ArtifactRuntimeManager {
    ArtifactRuntimeManager::new(ArtifactRuntimeManagerConfig::with_default_release(
        codex_home,
        versions::ARTIFACT_RUNTIME,
    ))
}

fn load_installed_runtime_from_env() -> Result<Option<InstalledArtifactRuntime>, FunctionCallError>
{
    let Some(root_dir) = runtime_path_override(ARTIFACT_RUNTIME_ROOT_ENV) else {
        return Ok(None);
    };
    let platform = ArtifactRuntimePlatform::detect_current().map_err(|error| {
        FunctionCallError::RespondToModel(format!(
            "failed to detect a supported platform for {ARTIFACT_RUNTIME_ROOT_ENV}: {error}"
        ))
    })?;
    let runtime = InstalledArtifactRuntime::load(root_dir.clone(), platform).map_err(|error| {
        FunctionCallError::RespondToModel(format!(
            "failed to load the artifact runtime from {} set via {ARTIFACT_RUNTIME_ROOT_ENV}: {error}",
            root_dir.display()
        ))
    })?;
    Ok(Some(runtime))
}

fn runtime_env_override(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn runtime_path_override(key: &str) -> Option<std::path::PathBuf> {
    runtime_env_override(key).map(std::path::PathBuf::from)
}

async fn emit_exec_begin(session: &Session, turn: &TurnContext, call_id: &str) {
    let emitter = ToolEmitter::shell(
        vec![ARTIFACTS_TOOL_NAME.to_string()],
        turn.cwd.clone(),
        ExecCommandSource::Agent,
        /*freeform*/ true,
    );
    let ctx = ToolEventCtx::new(session, turn, call_id, /*turn_diff_tracker*/ None);
    emitter.emit(ctx, ToolEventStage::Begin).await;
}

async fn emit_exec_end(
    session: &Session,
    turn: &TurnContext,
    call_id: &str,
    output: &ArtifactCommandOutput,
    duration: Duration,
    success: bool,
) {
    let exec_output = ExecToolCallOutput {
        exit_code: output.exit_code.unwrap_or(1),
        stdout: StreamOutput::new(output.stdout.clone()),
        stderr: StreamOutput::new(output.stderr.clone()),
        aggregated_output: StreamOutput::new(format_artifact_output(output)),
        duration,
        timed_out: false,
    };
    let emitter = ToolEmitter::shell(
        vec![ARTIFACTS_TOOL_NAME.to_string()],
        turn.cwd.clone(),
        ExecCommandSource::Agent,
        /*freeform*/ true,
    );
    let ctx = ToolEventCtx::new(session, turn, call_id, /*turn_diff_tracker*/ None);
    let stage = if success {
        ToolEventStage::Success(exec_output)
    } else {
        ToolEventStage::Failure(ToolEventFailure::Output(exec_output))
    };
    emitter.emit(ctx, stage).await;
}

fn format_artifact_output(output: &ArtifactCommandOutput) -> String {
    let stdout = output.stdout.trim();
    let stderr = output.stderr.trim();
    let mut sections = vec![format!(
        "exit_code: {}",
        output
            .exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "null".to_string())
    )];
    if !stdout.is_empty() {
        sections.push(format!("stdout:\n{stdout}"));
    }
    if !stderr.is_empty() {
        sections.push(format!("stderr:\n{stderr}"));
    }
    if stdout.is_empty() && stderr.is_empty() && output.success() {
        sections.push("artifact JS completed successfully.".to_string());
    }
    sections.join("\n\n")
}

fn error_output(error: &ArtifactsError) -> ArtifactCommandOutput {
    ArtifactCommandOutput {
        exit_code: Some(1),
        stdout: String::new(),
        stderr: error.to_string(),
    }
}

#[cfg(test)]
#[path = "artifacts_tests.rs"]
mod tests;
