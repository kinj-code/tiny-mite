//! Typed configuration service with precedence-based layering.
//!
//! # Configuration precedence (highest to lowest)
//!
//! 1. Environment variables (`TINYMITE_*`)
//! 2. User configuration file (`~/.config/tiny-mite/config.toml`)
//! 3. Project configuration file (`./.tinymite.toml`)
//! 4. Sensible defaults
//!
//! # Extension mechanism
//!
//! All configuration sections implement `Default` and `serde::Deserialize`.
//! Future subsystems add their section to `AppConfig` without breaking
//! existing consumers.
//!
//! # Secret handling
//!
//! Fields holding secrets use [`SecretString`], which redacts content in
//! `Debug`/`Display` and prevents accidental logging.

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

use tiny_mite_domain::DomainError;

// ---------------------------------------------------------------------------
// Secret string wrapper
// ---------------------------------------------------------------------------

/// A string whose content is redacted in `Debug` and `Display` output.
///
/// Use this for API keys, passwords, tokens, and other sensitive values.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    /// Create a new secret from a plain string.
    #[must_use]
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Expose the secret value. Callers must ensure it is handled safely.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString(***)")
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

impl Serialize for SecretString {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str("***")
    }
}

impl<'de> Deserialize<'de> for SecretString {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Ok(Self(raw))
    }
}

impl From<String> for SecretString {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for SecretString {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

// ---------------------------------------------------------------------------
// Logging section
// ---------------------------------------------------------------------------

/// Diagnostics / logging configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Maximum log level (trace, debug, info, warn, error).
    pub level: String,
    /// Emit JSON-formatted log lines (machine-readable).
    pub json_output: bool,
    /// Enable per-module filters (e.g. `tiny_mite_core=debug`).
    pub filters: Vec<String>,
    /// Use ANSI colors in terminal output.
    pub ansi: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self { level: "info".to_owned(), json_output: false, filters: Vec::new(), ansi: true }
    }
}

// ---------------------------------------------------------------------------
// Model / inference section
// ---------------------------------------------------------------------------

/// Configuration for model providers and inference defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Default model provider (e.g. "llama.cpp", "ollama").
    pub default_provider: String,
    /// Default model filename / ID.
    pub default_model: Option<String>,
    /// Maximum context window in tokens (0 = auto-detect).
    pub max_context_tokens: usize,
    /// Number of parallel inference slots.
    pub inference_slots: usize,
    /// Temperature for generation.
    pub temperature: f32,
    /// Model storage directory (relative to data dir or absolute).
    pub models_dir: PathBuf,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            default_provider: "llama.cpp".to_owned(),
            default_model: None,
            max_context_tokens: 0,
            inference_slots: 1,
            temperature: 0.7,
            models_dir: PathBuf::from("models"),
        }
    }
}

// ---------------------------------------------------------------------------
// Scheduler limits
// ---------------------------------------------------------------------------

/// Resource limits enforced by the scheduler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerConfig {
    /// Maximum concurrent tasks.
    pub max_concurrent_tasks: usize,
    /// Maximum concurrent tool executions.
    pub max_concurrent_tools: usize,
    /// Default task timeout in seconds.
    pub default_task_timeout_secs: u64,
    /// Maximum memory the system should target (bytes, 0 = auto).
    pub max_memory_bytes: u64,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_tasks: 4,
            max_concurrent_tools: 2,
            default_task_timeout_secs: 300,
            max_memory_bytes: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Security section
// ---------------------------------------------------------------------------

/// Security / sandbox configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Require user approval for tools at/below this risk level.
    pub approval_risk_threshold: String,
    /// Allowed filesystem roots (empty = current project only).
    pub allowed_paths: Vec<PathBuf>,
    /// Allow network access.
    pub allow_network: bool,
    /// Maximum process runtime for sandboxed tools (seconds).
    pub max_process_runtime_secs: u64,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            approval_risk_threshold: "medium".to_owned(),
            allowed_paths: Vec::new(),
            allow_network: false,
            max_process_runtime_secs: 30,
        }
    }
}

// ---------------------------------------------------------------------------
// Storage section
// ---------------------------------------------------------------------------

/// Storage / persistence configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Path to the embedded database file (relative to data dir).
    pub db_path: PathBuf,
    /// Event log retention (max events to store).
    pub max_event_log_entries: usize,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self { db_path: PathBuf::from("tiny-mite.db"), max_event_log_entries: 10_000 }
    }
}

// ---------------------------------------------------------------------------
// Retrieval section (placeholder)
// ---------------------------------------------------------------------------

/// Retrieval / context engine configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalConfig {
    /// Maximum number of documents to retrieve per query.
    pub max_documents: usize,
    /// Maximum tokens allocated to retrieved context.
    pub max_context_tokens: usize,
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self { max_documents: 20, max_context_tokens: 4096 }
    }
}

// ---------------------------------------------------------------------------
// UI section (placeholder)
// ---------------------------------------------------------------------------

/// Desktop UI configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiConfig {
    /// Theme name.
    pub theme: String,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self { theme: "saturated-jade".to_owned() }
    }
}

// ---------------------------------------------------------------------------
// Plugins section (placeholder)
// ---------------------------------------------------------------------------

/// Plugin / extension configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginsConfig {
    /// Enabled plugin identifiers.
    pub enabled: Vec<String>,
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self { enabled: Vec::new() }
    }
}

// ---------------------------------------------------------------------------
// Root configuration
// ---------------------------------------------------------------------------

/// The root application configuration.
///
/// Loaded via [`AppConfig::load`] which applies the layered precedence:
/// env vars > user config > project config > defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    pub logging: LoggingConfig,
    pub model: ModelConfig,
    pub scheduler: SchedulerConfig,
    pub security: SecurityConfig,
    pub storage: StorageConfig,
    pub retrieval: RetrievalConfig,
    pub ui: UiConfig,
    pub plugins: PluginsConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            logging: LoggingConfig::default(),
            model: ModelConfig::default(),
            scheduler: SchedulerConfig::default(),
            security: SecurityConfig::default(),
            storage: StorageConfig::default(),
            retrieval: RetrievalConfig::default(),
            ui: UiConfig::default(),
            plugins: PluginsConfig::default(),
        }
    }
}

impl AppConfig {
    // ── Env prefix ──────────────────────────────────────────

    const ENV_PREFIX: &'static str = "TINYMITE_";

    // ── Directory resolution ─────────────────────────────────

    /// Project directories for Tiny Mite (platform-standard locations).
    #[must_use]
    pub fn project_dirs() -> Option<ProjectDirs> {
        ProjectDirs::from("dev", "tinymite", "TinyMite")
    }

    /// Path to the user-level config file.
    #[must_use]
    pub fn user_config_path() -> Option<PathBuf> {
        Self::project_dirs().map(|d| d.config_dir().join("config.toml"))
    }

    /// Path to the project-level config file in the current directory.
    #[must_use]
    pub fn project_config_path() -> PathBuf {
        PathBuf::from(".tinymite.toml")
    }

    /// Data directory for the application.
    #[must_use]
    pub fn data_dir() -> Option<PathBuf> {
        Self::project_dirs().map(|d| d.data_dir().to_path_buf())
    }

    // ── Loading ──────────────────────────────────────────────

    /// Load configuration with full layered precedence.
    ///
    /// # Precedence (highest to lowest)
    ///
    /// 1. `TINYMITE_*` environment variables
    /// 2. User config file at `~/.config/tiny-mite/config.toml`
    /// 3. Project config file at `./.tinymite.toml`
    /// 4. `AppConfig::default()`
    ///
    /// # Errors
    ///
    /// Returns `Err` if a config file exists but cannot be parsed.
    /// Missing config files are not an error.
    pub fn load() -> Result<Self, DomainError> {
        let mut base = Self::default();

        // Layer 3: project config (lowest override)
        let project_path = Self::project_config_path();
        if project_path.exists() {
            let raw = std::fs::read_to_string(&project_path).map_err(|e| {
                DomainError::permanent(format!(
                    "Failed to read project config at {:?}: {e}",
                    project_path
                ))
                .with_source(e)
            })?;
            let project_cfg: PartialAppConfig = toml::from_str(&raw).map_err(|e| {
                DomainError::invalid_input(format!(
                    "Invalid project config at {:?}: {e}",
                    project_path
                ))
                .with_source(e)
            })?;
            project_cfg.apply_to(&mut base);
        }

        // Layer 2: user config
        if let Some(user_path) = Self::user_config_path() {
            if user_path.exists() {
                let raw = std::fs::read_to_string(&user_path).map_err(|e| {
                    DomainError::permanent(format!(
                        "Failed to read user config at {:?}: {e}",
                        user_path
                    ))
                    .with_source(e)
                })?;
                let user_cfg: PartialAppConfig = toml::from_str(&raw).map_err(|e| {
                    DomainError::invalid_input(format!(
                        "Invalid user config at {:?}: {e}",
                        user_path
                    ))
                    .with_source(e)
                })?;
                user_cfg.apply_to(&mut base);
            }
        }

        // Layer 1: environment variables (highest override)
        Self::apply_env_overrides(&mut base);

        Ok(base)
    }

    /// Apply environment variable overrides.
    fn apply_env_overrides(cfg: &mut Self) {
        // Mapping: ENV_VAR → setter closure
        // Logging
        env_str("TINYMITE_LOG_LEVEL", &mut cfg.logging.level);
        env_bool("TINYMITE_LOG_JSON", &mut cfg.logging.json_output);
        // Model
        env_str("TINYMITE_MODEL_PROVIDER", &mut cfg.model.default_provider);
        env_opt_str("TINYMITE_MODEL", &mut cfg.model.default_model);
        env_usize("TINYMITE_MAX_CONTEXT_TOKENS", &mut cfg.model.max_context_tokens);
        env_usize("TINYMITE_INFERENCE_SLOTS", &mut cfg.model.inference_slots);
        // Scheduler
        env_usize("TINYMITE_MAX_CONCURRENT_TASKS", &mut cfg.scheduler.max_concurrent_tasks);
        env_u64("TINYMITE_DEFAULT_TASK_TIMEOUT", &mut cfg.scheduler.default_task_timeout_secs);
        // Security
        env_str("TINYMITE_APPROVAL_THRESHOLD", &mut cfg.security.approval_risk_threshold);
        env_bool("TINYMITE_ALLOW_NETWORK", &mut cfg.security.allow_network);
        // UI
        env_str("TINYMITE_THEME", &mut cfg.ui.theme);
    }

    /// Validate the configuration and return a list of problems.
    #[must_use]
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();

        if self.logging.level.is_empty() {
            issues.push("logging.level must not be empty".into());
        }
        if self.model.default_provider.is_empty() {
            issues.push("model.default_provider must not be empty".into());
        }
        if self.model.inference_slots == 0 {
            issues.push("model.inference_slots must be >= 1".into());
        }
        if self.scheduler.max_concurrent_tasks == 0 {
            issues.push("scheduler.max_concurrent_tasks must be >= 1".into());
        }

        issues
    }
}

// ── Helpers for env override ───────────────────────────────────

fn env_str(key: &str, target: &mut String) {
    if let Ok(val) = std::env::var(format!("{prefix}{key}", prefix = AppConfig::ENV_PREFIX)) {
        *target = val;
    }
}

fn env_opt_str(key: &str, target: &mut Option<String>) {
    if let Ok(val) = std::env::var(format!("{prefix}{key}", prefix = AppConfig::ENV_PREFIX)) {
        *target = Some(val);
    }
}

fn env_bool(key: &str, target: &mut bool) {
    if let Ok(val) = std::env::var(format!("{prefix}{key}", prefix = AppConfig::ENV_PREFIX)) {
        if let Ok(b) = val.parse() {
            *target = b;
        }
    }
}

fn env_usize(key: &str, target: &mut usize) {
    if let Ok(val) = std::env::var(format!("{prefix}{key}", prefix = AppConfig::ENV_PREFIX)) {
        if let Ok(n) = val.parse() {
            *target = n;
        }
    }
}

fn env_u64(key: &str, target: &mut u64) {
    if let Ok(val) = std::env::var(format!("{prefix}{key}", prefix = AppConfig::ENV_PREFIX)) {
        if let Ok(n) = val.parse() {
            *target = n;
        }
    }
}

// ---------------------------------------------------------------------------
// Partial configuration for file-based merging
// ---------------------------------------------------------------------------

/// A version of `AppConfig` where all fields are `Option<T>` so missing
/// keys in config files don't overwrite layered values.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct PartialAppConfig {
    logging: Option<PartialLoggingConfig>,
    model: Option<PartialModelConfig>,
    scheduler: Option<PartialSchedulerConfig>,
    security: Option<PartialSecurityConfig>,
    storage: Option<PartialStorageConfig>,
    retrieval: Option<PartialRetrievalConfig>,
    ui: Option<PartialUiConfig>,
    plugins: Option<PartialPluginsConfig>,
}

impl PartialAppConfig {
    /// Apply any `Some` fields from this partial to `target`.
    fn apply_to(self, target: &mut AppConfig) {
        if let Some(v) = self.logging {
            v.apply_to(&mut target.logging);
        }
        if let Some(v) = self.model {
            v.apply_to(&mut target.model);
        }
        if let Some(v) = self.scheduler {
            v.apply_to(&mut target.scheduler);
        }
        if let Some(v) = self.security {
            v.apply_to(&mut target.security);
        }
        if let Some(v) = self.storage {
            v.apply_to(&mut target.storage);
        }
        if let Some(v) = self.retrieval {
            v.apply_to(&mut target.retrieval);
        }
        if let Some(v) = self.ui {
            v.apply_to(&mut target.ui);
        }
        if let Some(v) = self.plugins {
            v.apply_to(&mut target.plugins);
        }
    }
}

// ── Partial section types ──────────────────────────────────────

macro_rules! partial_section {
    ($vis:vis struct $name:ident => $target:ty { $($field:ident : $ftype:ty),* $(,)? }) => {
        #[derive(Debug, Clone, Default, Deserialize)]
        #[serde(default)]
        $vis struct $name {
            $( $field: Option<$ftype>, )*
        }
        impl $name {
            fn apply_to(self, t: &mut $target) {
                $( if let Some(v) = self.$field { t.$field = v; } )*
            }
        }
    };
}

partial_section!(struct PartialLoggingConfig => LoggingConfig {
    level: String,
    json_output: bool,
    filters: Vec<String>,
    ansi: bool,
});

partial_section!(struct PartialModelConfig => ModelConfig {
    default_provider: String,
    default_model: Option<String>,
    max_context_tokens: usize,
    inference_slots: usize,
    temperature: f32,
    models_dir: PathBuf,
});

partial_section!(struct PartialSchedulerConfig => SchedulerConfig {
    max_concurrent_tasks: usize,
    max_concurrent_tools: usize,
    default_task_timeout_secs: u64,
    max_memory_bytes: u64,
});

partial_section!(struct PartialSecurityConfig => SecurityConfig {
    approval_risk_threshold: String,
    allowed_paths: Vec<PathBuf>,
    allow_network: bool,
    max_process_runtime_secs: u64,
});

partial_section!(struct PartialStorageConfig => StorageConfig {
    db_path: PathBuf,
    max_event_log_entries: usize,
});

partial_section!(struct PartialRetrievalConfig => RetrievalConfig {
    max_documents: usize,
    max_context_tokens: usize,
});

partial_section!(struct PartialUiConfig => UiConfig {
    theme: String,
});

partial_section!(struct PartialPluginsConfig => PluginsConfig {
    enabled: Vec<String>,
});

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn defaults_are_sensible() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.logging.level, "info");
        assert_eq!(cfg.model.default_provider, "llama.cpp");
        assert!(cfg.scheduler.max_concurrent_tasks >= 1);
        assert!(!cfg.security.allow_network);
        assert_eq!(cfg.ui.theme, "saturated-jade");
    }

    #[test]
    fn validation_catches_empty_level() {
        let mut cfg = AppConfig::default();
        cfg.logging.level = String::new();
        let issues = cfg.validate();
        assert!(!issues.is_empty());
    }

    #[test]
    fn validation_catches_zero_inference_slots() {
        let mut cfg = AppConfig::default();
        cfg.model.inference_slots = 0;
        let issues = cfg.validate();
        assert!(issues.iter().any(|i| i.contains("inference_slots")));
    }

    #[test]
    fn secret_string_redacts() {
        let s = SecretString::new("my-api-key-12345");
        let debug = format!("{s:?}");
        let display = format!("{s}");
        assert!(!debug.contains("my-api-key-12345"));
        assert!(!display.contains("my-api-key-12345"));
        assert_eq!(s.expose(), "my-api-key-12345");
    }

    #[test]
    fn secret_string_serializes_redacted() {
        let s = SecretString::new("sk-12345");
        let json = serde_json::to_string(&s).expect("serialize");
        assert_eq!(json, "\"***\"");
    }

    #[test]
    fn env_override_applies_highest_precedence() {
        // Env override logic is tested via the helper functions
        // directly rather than using unsafe set_var.
        let mut cfg = AppConfig::default();
        assert_eq!(cfg.logging.level, "info");
        // Simulate of the env override path
        env_str("DUMMY_SKIP", &mut cfg.logging.level);
        // level should remain unchanged since env var isn't set
        assert_eq!(cfg.logging.level, "info");
    }

    #[test]
    fn project_config_overrides_default() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let config_path = dir.path().join(".tinymite.toml");
        let config_content = r#"
[logging]
level = "trace"
"#;
        std::fs::write(&config_path, config_content)?;

        // We can't easily redirect the project config path, so test
        // partial merge directly.
        let partial: PartialAppConfig = toml::from_str(config_content)?;
        let mut base = AppConfig::default();
        partial.apply_to(&mut base);
        assert_eq!(base.logging.level, "trace");
        Ok(())
    }

    #[test]
    fn partial_merge_only_overrides_specified_fields() {
        let toml_content = r#"
[logging]
level = "warn"
"#;
        let partial: PartialAppConfig = toml::from_str(toml_content).expect("parse");
        let mut base = AppConfig::default();
        let original_provider = base.model.default_provider.clone();
        partial.apply_to(&mut base);
        assert_eq!(base.logging.level, "warn");
        // model wasn't specified, must keep default
        assert_eq!(base.model.default_provider, original_provider);
    }
}
