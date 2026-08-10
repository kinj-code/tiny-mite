//! Native llama.cpp provider with verified-ABI dynamic FFI.
//!
//! # ABI status
//!
//! Struct layouts verified by standalone C probe against llama.h v0.1.152.
//! `llama_model_default_params()` is called via sret convention correctly.
//!
//! Currently validates: backend_init, model load, context create, tokenize, cleanup.
//! Generation/streaming will be re-enabled once model loading is confirmed.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tiny_mite_domain::ModelId;
use tiny_mite_runtime::{
    Backend, DeviceInfo, InferenceRequest, InferenceResponse, ModelCapabilities, ModelInfo,
    ModelProvider, ModelState, ProviderError,
};
use tokio::sync::Mutex;

use crate::ffi::{self, LlamaError, OpaquePtr};

struct LoadedModel {
    info: ModelInfo,
    model: Option<OpaquePtr>,
    context: Option<OpaquePtr>,
}

use std::fmt;
impl fmt::Debug for LoadedModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoadedModel").field("info", &self.info).finish()
    }
}

#[derive(Clone)]
pub struct NativeLlamaCppProvider {
    models_dir: String,
    initialized: Arc<AtomicBool>,
    backend: Backend,
    n_threads: usize,
    loaded: Arc<Mutex<HashMap<ModelId, Arc<LoadedModel>>>>,
}

impl fmt::Debug for NativeLlamaCppProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeLlamaCppProvider")
            .field("models_dir", &self.models_dir)
            .field("backend", &self.backend)
            .field("n_threads", &self.n_threads)
            .finish()
    }
}

impl NativeLlamaCppProvider {
    #[must_use]
    pub fn new(models_dir: impl Into<String>, backend: Backend) -> Self {
        Self {
            models_dir: models_dir.into(),
            initialized: Arc::new(AtomicBool::new(false)),
            backend,
            n_threads: 4,
            loaded: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[must_use]
    pub fn with_threads(mut self, n: usize) -> Self {
        self.n_threads = n;
        self
    }

    fn ensure_init(&self) -> Result<(), ProviderError> {
        if !self.initialized.swap(true, Ordering::AcqRel) {
            ffi::backend_init()
                .map_err(|e| ProviderError::Internal(format!("llama.cpp init: {e}")))?;
        }
        Ok(())
    }

    fn scan_dir(&self, dir: &str, models: &mut Vec<ModelInfo>) -> Result<(), ProviderError> {
        let entries = std::fs::read_dir(dir)
            .map_err(|e| ProviderError::Internal(format!("Cannot read models directory: {e}")))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_symlink() {
                continue;
            }
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if !name.starts_with('.') {
                        self.scan_dir(&path.to_string_lossy(), models)?;
                    }
                }
                continue;
            }
            if !path.extension().map(|e| e == "gguf").unwrap_or(false) {
                continue;
            }
            match crate::gguf::inspect_gguf(&path) {
                Ok(meta) => {
                    let id = ModelId::new();
                    models.push(ModelInfo {
                        id,
                        provider: "llama.cpp".into(),
                        name: meta.name.clone().unwrap_or_else(|| {
                            path.file_stem()
                                .map(|s| s.to_string_lossy().into())
                                .unwrap_or("unknown".into())
                        }),
                        family: meta.family.clone(),
                        architecture: meta.architecture.clone(),
                        format: Some("gguf".into()),
                        quantization: meta.quantization.clone(),
                        parameter_size_billions: meta.parameter_count.map(|p| p as f32 / 1e9),
                        max_context_length: meta.context_length.unwrap_or(0) as usize,
                        file_path: Some(path.to_string_lossy().to_string()),
                        capabilities: ModelCapabilities {
                            text_generation: true,
                            chat: true,
                            ..Default::default()
                        },
                        estimated_ram_bytes: meta.estimated_ram_bytes,
                        state: ModelState::Validated,
                    });
                }
                Err(e) => {
                    tracing::warn!(path=%path.display(), error=%e, "GGUF inspect failed");
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl ModelProvider for NativeLlamaCppProvider {
    fn name(&self) -> &'static str {
        "llama.cpp"
    }
    fn provider_capabilities(&self) -> ModelCapabilities {
        ModelCapabilities { text_generation: true, chat: true, ..Default::default() }
    }

    async fn discover_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        self.ensure_init()?;
        let mut models = Vec::new();
        self.scan_dir(&self.models_dir, &mut models)?;
        Ok(models)
    }

    async fn inspect(&self, id: &ModelId) -> Result<ModelInfo, ProviderError> {
        let models = self.discover_models().await?;
        models.into_iter().find(|m| &m.id == id).ok_or(ProviderError::NotFound(*id))
    }

    async fn load(&self, id: &ModelId) -> Result<ModelInfo, ProviderError> {
        self.ensure_init()?;
        let models = self.discover_models().await?;
        let info = models.into_iter().find(|m| &m.id == id).ok_or(ProviderError::NotFound(*id))?;

        let path_str = info
            .file_path
            .as_ref()
            .ok_or_else(|| ProviderError::Internal("Model has no file path".into()))?;
        let path = std::path::Path::new(path_str);

        let model = ffi::load_model(path, 0, true)
            .map_err(|e| ProviderError::Internal(format!("Model load: {e}")))?;
        let ctx_len =
            if info.max_context_length > 0 { info.max_context_length as u32 } else { 2048 };
        let context = ffi::create_context(&model, ctx_len, self.n_threads as i32)
            .map_err(|e| ProviderError::Internal(format!("Context create: {e}")))?;

        let loaded = Arc::new(LoadedModel {
            info: ModelInfo { state: ModelState::Ready, ..info.clone() },
            model: Some(model),
            context: Some(context),
        });
        self.loaded.lock().await.insert(*id, loaded.clone());
        Ok(loaded.info.clone())
    }

    async fn unload(&self, id: &ModelId) -> Result<(), ProviderError> {
        let mut loaded_map = self.loaded.lock().await;
        if let Some(entry) = loaded_map.remove(id) {
            if let Some(ctx) = entry.context {
                ffi::free_context(ctx);
            }
            if let Some(model) = entry.model {
                ffi::free_model(model);
            }
        }
        Ok(())
    }

    async fn generate(
        &self,
        _request: &InferenceRequest,
    ) -> Result<InferenceResponse, ProviderError> {
        Err(ProviderError::Internal("Generation not yet implemented after ABI fix".into()))
    }

    async fn stream(
        &self,
        _request: &InferenceRequest,
        _sink: tokio::sync::mpsc::Sender<InferenceResponse>,
    ) -> Result<(), ProviderError> {
        Err(ProviderError::Internal("Streaming not yet implemented after ABI fix".into()))
    }

    async fn cancel(&self, _correlation_id: &str) -> Result<(), ProviderError> {
        Ok(())
    }
    async fn health_check(&self) -> Result<(), ProviderError> {
        self.ensure_init()?;
        Ok(())
    }

    async fn list_devices(&self) -> Result<Vec<DeviceInfo>, ProviderError> {
        Ok(vec![DeviceInfo {
            device_id: 0,
            name: "CPU — llama.cpp (verified ABI)".into(),
            backend: self.backend,
            memory_bytes: 0,
            available: true,
            recommended: true,
        }])
    }

    async fn count_tokens(&self, model_id: &ModelId, text: &str) -> Result<usize, ProviderError> {
        let loaded_map = self.loaded.lock().await;
        let loaded = loaded_map.get(model_id).ok_or_else(|| ProviderError::NotLoaded(*model_id))?;
        let model = loaded.model.as_ref().ok_or_else(|| ProviderError::NotLoaded(*model_id))?;
        ffi::token_count(model, text)
            .map_err(|e| ProviderError::Internal(format!("Token count: {e}")))
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn provider_name_is_llama_cpp() {
        assert_eq!(NativeLlamaCppProvider::new("/tmp/models", Backend::Cpu).name(), "llama.cpp");
    }
    #[tokio::test]
    async fn provider_capability_text_generation() {
        assert!(
            NativeLlamaCppProvider::new("/tmp/models", Backend::Cpu)
                .provider_capabilities()
                .text_generation
        );
    }
    #[tokio::test]
    async fn provider_discover_returns_error_for_nonexistent_dir() {
        assert!(
            NativeLlamaCppProvider::new("/tmp/nonexistent_gguf_dir_12345", Backend::Cpu)
                .discover_models()
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn provider_inspect_not_found() {
        assert!(
            NativeLlamaCppProvider::new("/tmp/models", Backend::Cpu)
                .inspect(&ModelId::new())
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn provider_health_check_passes() {
        NativeLlamaCppProvider::new("/tmp/models", Backend::Cpu).health_check().await.unwrap();
    }
    #[tokio::test]
    async fn provider_list_devices() {
        assert!(
            !NativeLlamaCppProvider::new("/tmp/models", Backend::Cpu)
                .list_devices()
                .await
                .unwrap()
                .is_empty()
        );
    }
}
