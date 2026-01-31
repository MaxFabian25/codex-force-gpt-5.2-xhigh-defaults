use crate::config::Config;
use crate::protocol::SandboxPolicy;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::config_types::Verbosity;
use codex_protocol::openai_models::ReasoningEffort;
use serde::Deserialize;
use serde::Serialize;

/// Base instructions for the orchestrator role.
const ORCHESTRATOR_PROMPT: &str = include_str!("../../templates/agents/orchestrator.md");

/// Default model override used by spawned sub-agents.
const DEFAULT_SUBAGENT_MODEL: &str = "gpt-5.2";
const DEFAULT_SUBAGENT_REASONING_EFFORT: ReasoningEffort = ReasoningEffort::XHigh;
const DEFAULT_SUBAGENT_VERBOSITY: Verbosity = Verbosity::High;
const DEFAULT_SUBAGENT_REASONING_SUMMARY: ReasoningSummary = ReasoningSummary::Detailed;
const DEFAULT_SUBAGENT_SUPPORTS_REASONING_SUMMARIES: bool = true;

/// Enumerated list of all supported agent roles.
const ALL_ROLES: [AgentRole; 4] = [
    AgentRole::Default,
    AgentRole::Orchestrator,
    AgentRole::Worker,
    AgentRole::Explorer,
];

/// Hard-coded agent role selection used when spawning sub-agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    /// Inherit the parent agent's configuration unchanged.
    Default,
    /// Coordination-only agent that delegates to workers.
    Orchestrator,
    /// Task-executing agent with a fixed model override.
    Worker,
    /// Task-executing agent with a fixed model override.
    Explorer,
}

/// Immutable profile data that drives per-agent configuration overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AgentProfile {
    /// Optional base instructions override.
    pub base_instructions: Option<&'static str>,
    /// Optional model override.
    pub model: Option<&'static str>,
    /// Optional reasoning effort override.
    pub reasoning_effort: Option<ReasoningEffort>,
    /// Optional verbosity override.
    pub verbosity: Option<Verbosity>,
    /// Optional reasoning summary override.
    pub reasoning_summary: Option<ReasoningSummary>,
    /// Optional override to force-enable reasoning summaries.
    pub supports_reasoning_summaries: Option<bool>,
    /// Whether to force a read-only sandbox policy.
    pub read_only: bool,
    /// Description to include in the tool specs.
    pub description: &'static str,
}

impl AgentRole {
    /// Returns the string values used by JSON schema enums.
    pub fn enum_values() -> Vec<String> {
        ALL_ROLES
            .iter()
            .filter_map(|role| {
                let description = role.profile().description;
                serde_json::to_string(role)
                    .map(|role| {
                        let description = if !description.is_empty() {
                            format!(r#", "description": {description}"#)
                        } else {
                            String::new()
                        };
                        format!(r#"{{ "name": {role}{description}}}"#)
                    })
                    .ok()
            })
            .collect()
    }

    /// Returns the hard-coded profile for this role.
    pub fn profile(self) -> AgentProfile {
        match self {
            AgentRole::Default => AgentProfile::default(),
            AgentRole::Orchestrator => AgentProfile {
                base_instructions: Some(ORCHESTRATOR_PROMPT),
                model: Some(DEFAULT_SUBAGENT_MODEL),
                reasoning_effort: Some(DEFAULT_SUBAGENT_REASONING_EFFORT),
                verbosity: Some(DEFAULT_SUBAGENT_VERBOSITY),
                reasoning_summary: Some(DEFAULT_SUBAGENT_REASONING_SUMMARY),
                supports_reasoning_summaries: Some(DEFAULT_SUBAGENT_SUPPORTS_REASONING_SUMMARIES),
                ..Default::default()
            },
            AgentRole::Worker => AgentProfile {
                // base_instructions: Some(WORKER_PROMPT),
                // model: Some(WORKER_MODEL),
                model: Some(DEFAULT_SUBAGENT_MODEL),
                reasoning_effort: Some(DEFAULT_SUBAGENT_REASONING_EFFORT),
                verbosity: Some(DEFAULT_SUBAGENT_VERBOSITY),
                reasoning_summary: Some(DEFAULT_SUBAGENT_REASONING_SUMMARY),
                supports_reasoning_summaries: Some(DEFAULT_SUBAGENT_SUPPORTS_REASONING_SUMMARIES),
                description: r#"Use for execution and production work.
Typical tasks:
- Implement part of a feature
- Fix tests or bugs
- Split large refactors into independent chunks
Rules:
- Explicitly assign **ownership** of the task (files / responsibility).
- Always tell workers they are **not alone in the codebase**, and they should ignore edits made by others without touching them"#,
                ..Default::default()
            },
            AgentRole::Explorer => AgentProfile {
                model: Some(DEFAULT_SUBAGENT_MODEL),
                reasoning_effort: Some(DEFAULT_SUBAGENT_REASONING_EFFORT),
                verbosity: Some(DEFAULT_SUBAGENT_VERBOSITY),
                reasoning_summary: Some(DEFAULT_SUBAGENT_REASONING_SUMMARY),
                supports_reasoning_summaries: Some(DEFAULT_SUBAGENT_SUPPORTS_REASONING_SUMMARIES),
                description: r#"Use `explorer` for all codebase questions.
Explorers are fast and authoritative.
Always prefer them over manual search or file reading.
Rules:
- Ask explorers first and precisely.
- Do not re-read or re-search code they cover.
- Trust explorer results without verification.
- Run explorers in parallel when useful.
- Reuse existing explorers for related questions.
                "#,
                ..Default::default()
            },
        }
    }

    /// Applies this role's profile onto the provided config.
    pub fn apply_to_config(self, config: &mut Config) -> Result<(), String> {
        let profile = self.profile();
        if let Some(base_instructions) = profile.base_instructions {
            config.base_instructions = Some(base_instructions.to_string());
        }
        if let Some(model) = profile.model {
            config.model = Some(model.to_string());
        }
        if let Some(reasoning_effort) = profile.reasoning_effort {
            config.model_reasoning_effort = Some(reasoning_effort);
        }
        if let Some(verbosity) = profile.verbosity {
            config.model_verbosity = Some(verbosity);
        }
        if let Some(reasoning_summary) = profile.reasoning_summary {
            config.model_reasoning_summary = reasoning_summary;
        }
        if let Some(supports_reasoning_summaries) = profile.supports_reasoning_summaries {
            config.model_supports_reasoning_summaries = Some(supports_reasoning_summaries);
        }
        if profile.read_only {
            config
                .sandbox_policy
                .set(SandboxPolicy::new_read_only_policy())
                .map_err(|err| format!("sandbox_policy is invalid: {err}"))?;
        }
        Ok(())
    }
}
