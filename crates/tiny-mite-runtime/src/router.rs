//! Model router — selects the best provider for a given task.
//!
//! The router uses task analysis and model capabilities to select
//! an appropriate provider. It supports provider health checks,
//! fallback chains, and capability-based filtering.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use tiny_mite_domain::ModelId;

use crate::inference::{InferenceRequest, InferenceResponse};
use crate::model::{Backend, DeviceInfo, ModelCapabilities, ModelInfo, ModelState};
use crate::provider::{ModelProvider, ProviderError};

/// A registered provider with health status.
#[derive(Debug)]
struct ProviderEntry {
    provider: Box<dyn ModelProvider>,
    healthy: bool,
    last_checked: chrono::DateTime<chrono::Utc>,
    models: Vec<ModelInfo>,
}

/// Routes inference requests to compatible providers.
///
/// Providers are checked for health, models are discovered, and
/// the best match is selected based on capabilities.
pub struct ModelRouter {
    providers: RwLock<HashMap<String, ProviderEntry>>,
    /// Fallback order: preferred provider name first.
    preference_order: Vec<String>,
}

impl ModelRouter {
    /// Create a new router.
    #[must_use]
    pub fn new() -> Self {
        Self { providers: RwLock::new(HashMap::new()), preference_order: Vec::new() }
    }

    /// Register a provider.
    pub async fn register(&mut self, name: impl Into<String>, provider: Box<dyn ModelProvider>) {
        let name = name.into();
        let mut providers = self.providers.write().await;
        providers.insert(
            name.clone(),
            ProviderEntry {
                provider,
                healthy: true,
                last_checked: chrono::Utc::now(),
                models: Vec::new(),
            },
        );
        self.preference_order.push(name);
    }

    /// Find a provider that supports the requested capabilities.
    /// Returns the provider name and capabilities rather than a reference
    /// to avoid borrowing issues with the internal RwLock.
    pub async fn find_provider_name(
        &self,
        required_capabilities: &ModelCapabilities,
    ) -> Option<String> {
        let providers = self.providers.read().await;

        for name in &self.preference_order {
            if let Some(entry) = providers.get(name) {
                if entry.healthy {
                    let caps = entry.provider.provider_capabilities();
                    if Self::satisfies(&caps, required_capabilities) {
                        return Some(name.clone());
                    }
                }
            }
        }

        for (name, entry) in providers.iter() {
            if entry.healthy {
                let caps = entry.provider.provider_capabilities();
                if Self::satisfies(&caps, required_capabilities) {
                    return Some(name.clone());
                }
            }
        }

        None
    }

    /// Check if a named provider is healthy and satisfies requirements.
    pub async fn check_provider(&self, name: &str, required: &ModelCapabilities) -> bool {
        let providers = self.providers.read().await;
        if let Some(entry) = providers.get(name) {
            if entry.healthy {
                return Self::satisfies(&entry.provider.provider_capabilities(), required);
            }
        }
        false
    }

    /// Check if provider capabilities satisfy requirements.
    fn satisfies(have: &ModelCapabilities, need: &ModelCapabilities) -> bool {
        if need.text_generation && !have.text_generation {
            return false;
        }
        if need.chat && !have.chat {
            return false;
        }
        if need.tool_calling && !have.tool_calling {
            return false;
        }
        if need.embeddings && !have.embeddings {
            return false;
        }
        if need.reasoning && !have.reasoning {
            return false;
        }
        true
    }

    /// Run health checks on all providers.
    pub async fn health_check_all(&self) {
        let mut providers = self.providers.write().await;
        for entry in providers.values_mut() {
            match entry.provider.health_check().await {
                Ok(()) => {
                    entry.healthy = true;
                    entry.last_checked = chrono::Utc::now();
                }
                Err(_) => {
                    entry.healthy = false;
                }
            }
        }
    }

    /// Generate a response using the best matching healthy provider.
    ///
    /// Finds a provider that satisfies `required_capabilities`, then
    /// delegates the inference call to it.
    pub async fn generate(
        &self,
        required_capabilities: &ModelCapabilities,
        request: &InferenceRequest,
    ) -> Result<InferenceResponse, ProviderError> {
        let name = self
            .find_provider_name(required_capabilities)
            .await
            .ok_or_else(|| ProviderError::Internal("No matching healthy provider found".into()))?;

        let providers = self.providers.read().await;
        let entry = providers
            .get(&name)
            .ok_or_else(|| ProviderError::Internal(format!("Provider '{name}' disappeared")))?;

        entry.provider.generate(request).await
    }

    /// Get healthy provider count.
    pub async fn healthy_count(&self) -> usize {
        let providers = self.providers.read().await;
        providers.values().filter(|e| e.healthy).count()
    }

    /// List all registered provider names.
    pub async fn provider_names(&self) -> Vec<String> {
        let providers = self.providers.read().await;
        providers.keys().cloned().collect()
    }

    /// Set preference order for provider selection.
    pub async fn set_preference(&mut self, order: Vec<String>) {
        self.preference_order = order;
    }
}

impl Default for ModelRouter {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::{InferenceRequest, InferenceResponse};
    use async_trait::async_trait;

    #[derive(Debug, Clone)]
    struct MockProvider {
        name: &'static str,
        caps: ModelCapabilities,
        healthy: bool,
    }

    #[async_trait]
    impl ModelProvider for MockProvider {
        fn name(&self) -> &'static str {
            self.name
        }
        fn provider_capabilities(&self) -> ModelCapabilities {
            self.caps.clone()
        }

        async fn discover_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            Ok(Vec::new())
        }
        async fn inspect(&self, _id: &ModelId) -> Result<ModelInfo, ProviderError> {
            Err(ProviderError::NotFound(ModelId::new()))
        }
        async fn load(&self, _id: &ModelId) -> Result<ModelInfo, ProviderError> {
            Err(ProviderError::Internal("mock".into()))
        }
        async fn unload(&self, _id: &ModelId) -> Result<(), ProviderError> {
            Ok(())
        }
        async fn generate(
            &self,
            _request: &InferenceRequest,
        ) -> Result<InferenceResponse, ProviderError> {
            Err(ProviderError::Internal("mock".into()))
        }
        async fn stream(
            &self,
            _request: &InferenceRequest,
            _sink: tokio::sync::mpsc::Sender<InferenceResponse>,
        ) -> Result<(), ProviderError> {
            Ok(())
        }
        async fn cancel(&self, _id: &str) -> Result<(), ProviderError> {
            Ok(())
        }
        async fn health_check(&self) -> Result<(), ProviderError> {
            if self.healthy { Ok(()) } else { Err(ProviderError::Internal("unhealthy".into())) }
        }
        async fn list_devices(&self) -> Result<Vec<DeviceInfo>, ProviderError> {
            Ok(Vec::new())
        }
        async fn count_tokens(
            &self,
            _model_id: &ModelId,
            _text: &str,
        ) -> Result<usize, ProviderError> {
            Ok(0)
        }
    }

    #[tokio::test]
    async fn router_finds_capable_provider() {
        let mut router = ModelRouter::new();
        router
            .register(
                "ollama",
                Box::new(MockProvider {
                    name: "ollama",
                    caps: ModelCapabilities {
                        text_generation: true,
                        chat: true,
                        tool_calling: false,
                        ..Default::default()
                    },
                    healthy: true,
                }),
            )
            .await;

        let need = ModelCapabilities { text_generation: true, ..Default::default() };
        let found = router.find_provider(&need).await;
        assert!(found.is_some());
    }

    #[tokio::test]
    async fn router_skips_unhealthy_provider() {
        let mut router = ModelRouter::new();
        router
            .register(
                "bad",
                Box::new(MockProvider {
                    name: "bad",
                    caps: ModelCapabilities { text_generation: true, ..Default::default() },
                    healthy: false,
                }),
            )
            .await;

        let need = ModelCapabilities { text_generation: true, ..Default::default() };
        let found = router.find_provider(&need).await;
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn router_filters_by_capabilities() {
        let mut router = ModelRouter::new();
        router
            .register(
                "chat_only",
                Box::new(MockProvider {
                    name: "chat_only",
                    caps: ModelCapabilities {
                        text_generation: true,
                        chat: true,
                        ..Default::default()
                    },
                    healthy: true,
                }),
            )
            .await;
        router
            .register(
                "tool_capable",
                Box::new(MockProvider {
                    name: "tool_capable",
                    caps: ModelCapabilities {
                        text_generation: true,
                        chat: true,
                        tool_calling: true,
                        ..Default::default()
                    },
                    healthy: true,
                }),
            )
            .await;
        router.set_preference(vec!["tool_capable".into(), "chat_only".into()]).await;

        let need =
            ModelCapabilities { text_generation: true, tool_calling: true, ..Default::default() };
        let found = router.find_provider(&need).await;
        assert_eq!(found.unwrap().0, "tool_capable");
    }

    #[tokio::test]
    async fn health_check_marks_unhealthy() {
        let mut router = ModelRouter::new();
        router
            .register(
                "flaky",
                Box::new(MockProvider {
                    name: "flaky",
                    caps: ModelCapabilities { text_generation: true, ..Default::default() },
                    healthy: false,
                }),
            )
            .await;

        router.health_check_all().await;
        assert_eq!(router.healthy_count().await, 0);
    }
}
