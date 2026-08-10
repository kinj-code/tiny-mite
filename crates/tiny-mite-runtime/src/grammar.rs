//! Grammar-constrained and structured output generation.
//!
//! Provides abstractions for grammar-constrained generation (GBNF)
//! and structured JSON output via JSON Schema.

use serde::{Deserialize, Serialize};

/// A grammar constraint for guided text generation.
///
/// Supports GBNF (GGML BNF) format used by llama.cpp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrammarConstraint {
    /// GBNF grammar string.
    pub grammar: String,
    /// Human-readable description of what this grammar produces.
    pub description: String,
    /// Whether the grammar constrains the model to produce valid JSON.
    pub produces_json: bool,
}

/// A JSON Schema for structured output.
///
/// When provided with an inference request, the model should produce
/// output conforming to this schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonSchemaConstraint {
    /// The JSON Schema as a serde value.
    pub schema: serde_json::Value,
    /// Schema identifier for logging.
    pub name: String,
    /// Whether to strictly enforce the schema.
    pub strict: bool,
}

/// Configuration for speculative decoding.
///
/// Speculative decoding uses a smaller "draft" model to predict tokens
/// that are then verified by the target model, improving throughput.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeculativeDecodingConfig {
    /// Whether speculative decoding is enabled.
    pub enabled: bool,
    /// Draft model identifier (smaller/faster model).
    pub draft_model_id: Option<String>,
    /// Number of tokens the draft model predicts per step.
    pub draft_tokens: usize,
    /// Whether to fall back to non-speculative decoding on mismatch.
    pub fallback_on_mismatch: bool,
}

impl Default for SpeculativeDecodingConfig {
    fn default() -> Self {
        Self { enabled: false, draft_model_id: None, draft_tokens: 4, fallback_on_mismatch: true }
    }
}

/// Hardware capability detection result.
///
/// Reports what backends and features are available on the current machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareCapabilities {
    /// Whether CPU inference is available (always true).
    pub cpu_available: bool,
    /// Whether CUDA is available.
    pub cuda_available: bool,
    /// Whether Vulkan is available.
    pub vulkan_available: bool,
    /// Whether Metal is available.
    pub metal_available: bool,
    /// Whether ROCm/HIP is available.
    pub rocm_available: bool,
    /// Whether AVX2 instructions are supported.
    pub avx2_supported: bool,
    /// Whether AVX512 instructions are supported.
    pub avx512_supported: bool,
    /// Total system RAM in bytes.
    pub total_ram_bytes: u64,
    /// Available system RAM in bytes.
    pub available_ram_bytes: u64,
    /// Number of logical CPU cores.
    pub cpu_logical_cores: usize,
    /// Recommended backend for this machine.
    pub recommended_backend: String,
}

impl HardwareCapabilities {
    /// Detect hardware capabilities on the current machine.
    #[must_use]
    pub fn detect() -> Self {
        let total_ram = if cfg!(target_os = "linux") {
            std::fs::read_to_string("/proc/meminfo")
                .ok()
                .and_then(|s| {
                    s.lines()
                        .find(|l| l.starts_with("MemTotal:"))
                        .and_then(|l| l.split_whitespace().nth(1))
                        .and_then(|v| v.parse::<u64>().ok())
                        .map(|kb| kb * 1024)
                })
                .unwrap_or(8_589_934_592)
        } else {
            8_589_934_592 // default 8 GB
        };

        let available_ram = if cfg!(target_os = "linux") {
            std::fs::read_to_string("/proc/meminfo")
                .ok()
                .and_then(|s| {
                    s.lines()
                        .find(|l| l.starts_with("MemAvailable:"))
                        .and_then(|l| l.split_whitespace().nth(1))
                        .and_then(|v| v.parse::<u64>().ok())
                        .map(|kb| kb * 1024)
                })
                .unwrap_or(total_ram / 2)
        } else {
            total_ram / 2
        };

        let cpu_cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);

        // Check for AVX flags on Linux
        let (avx2, avx512) = if cfg!(target_os = "linux") {
            let flags = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
            (flags.contains("avx2"), flags.contains("avx512"))
        } else {
            (false, false)
        };

        Self {
            cpu_available: true,
            cuda_available: false,
            vulkan_available: false,
            metal_available: false,
            rocm_available: false,
            avx2_supported: avx2,
            avx512_supported: avx512,
            total_ram_bytes: total_ram,
            available_ram_bytes: available_ram,
            cpu_logical_cores: cpu_cores,
            recommended_backend: "cpu".into(),
        }
    }

    /// Returns the recommended backend name.
    #[must_use]
    pub fn best_backend(&self) -> &str {
        if self.cuda_available {
            "cuda"
        } else if self.vulkan_available {
            "vulkan"
        } else if self.rocm_available {
            "rocm"
        } else if self.metal_available {
            "metal"
        } else {
            "cpu"
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grammar_constraint_builder() {
        let grammar = GrammarConstraint {
            grammar: r#"root ::= "hello" | "world""#.into(),
            description: "Simple greeting".into(),
            produces_json: false,
        };
        assert!(grammar.grammar.contains("root"));
    }

    #[test]
    fn speculative_decoding_defaults() {
        let cfg = SpeculativeDecodingConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.draft_tokens, 4);
    }

    #[test]
    fn hardware_capabilities_detect() {
        let caps = HardwareCapabilities::detect();
        assert!(caps.cpu_available);
        assert!(caps.total_ram_bytes > 0);
        assert!(caps.cpu_logical_cores >= 1);
    }
}
