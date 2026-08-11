//! Provider adapters — real HTTP implementations for Ollama, LM Studio, and OpenAI.
//!
//! Each adapter wraps a remote or local server behind the standard
//! [`ModelProvider`](crate::ModelProvider) trait.
//!
//! # Streaming
//!
//! Providers supporting streaming parse incremental chunks and emit
//! token/text deltas through `StreamingSession`.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tiny_mite_domain::ModelId;
use tokio::sync::mpsc;

use crate::inference::{InferenceRequest, InferenceResponse};
use crate::model::{Backend, DeviceInfo, ModelCapabilities, ModelInfo, ModelState};
use crate::provider::{ModelProvider, ProviderError};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

// ── Helper ────────────────────────────────────────────────────────

fn build_client() -> Result<Client, reqwest::Error> {
    Client::builder().connect_timeout(CONNECT_TIMEOUT).timeout(DEFAULT_TIMEOUT).build()
}

// ── Ollama provider ───────────────────────────────────────────────

/// Provider that connects to a local Ollama instance at `http://localhost:11434`.
pub struct OllamaProvider {
    pub base_url: String,
    client: Client,
}

impl std::fmt::Debug for OllamaProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OllamaProvider").field("base_url", &self.base_url).finish()
    }
}

impl OllamaProvider {
    #[must_use]
    pub fn new(base_url: impl Into<String>) -> Self {
        Self { base_url: base_url.into(), client: build_client().expect("reqwest client") }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url.trim_end_matches('/'))
    }
}

#[async_trait]
impl ModelProvider for OllamaProvider {
    fn name(&self) -> &'static str {
        "ollama"
    }

    fn provider_capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            text_generation: true,
            chat: true,
            tool_calling: true,
            structured_output: false,
            embeddings: true,
            reranking: false,
            vision: true,
            audio: false,
            reasoning: false,
            speculative_decoding: false,
            grammar_constrained_output: false,
        }
    }

    async fn discover_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let resp = self
            .client
            .get(self.url("/api/tags"))
            .send()
            .await
            .map_err(|e| ProviderError::Internal(format!("Ollama connect: {e}")))?;

        let body: OllamaTagsResp =
            resp.json().await.map_err(|e| ProviderError::Internal(format!("Ollama parse: {e}")))?;

        Ok(body
            .models
            .unwrap_or_default()
            .into_iter()
            .map(|m| ModelInfo {
                id: ModelId::new(),
                provider: "ollama".into(),
                name: m.name,
                family: None,
                architecture: None,
                format: Some("gguf".into()),
                quantization: None,
                parameter_size_billions: None,
                max_context_length: 4096,
                file_path: None,
                capabilities: self.provider_capabilities(),
                estimated_ram_bytes: 0,
                state: ModelState::Discovered,
            })
            .collect())
    }

    async fn inspect(&self, _id: &ModelId) -> Result<ModelInfo, ProviderError> {
        Err(ProviderError::Unsupported("Ollama inspect not available".into()))
    }

    async fn load(&self, _id: &ModelId) -> Result<ModelInfo, ProviderError> {
        Ok(ModelInfo {
            id: ModelId::new(),
            provider: "ollama".into(),
            name: "loaded-model".into(),
            family: None,
            architecture: None,
            format: None,
            quantization: None,
            parameter_size_billions: None,
            max_context_length: 4096,
            file_path: None,
            capabilities: self.provider_capabilities(),
            estimated_ram_bytes: 0,
            state: ModelState::Ready,
        })
    }

    async fn unload(&self, _id: &ModelId) -> Result<(), ProviderError> {
        Ok(())
    }

    async fn generate(
        &self,
        request: &InferenceRequest,
    ) -> Result<InferenceResponse, ProviderError> {
        let start = std::time::Instant::now();
        let body = OllamaGenerateReq {
            model: request.model_name.clone(),
            prompt: request.prompt.clone(),
            stream: false,
        };

        let resp = self
            .client
            .post(self.url("/api/generate"))
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Internal(format!("Ollama generate: {e}")))?;

        let gen_resp: OllamaGenerateResp = resp
            .json()
            .await
            .map_err(|e| ProviderError::Internal(format!("Ollama parse response: {e}")))?;

        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        Ok(InferenceResponse {
            id: request.correlation_id.map_or("".into(), |c| c.to_string()),
            model_id: request.model_id,
            text: gen_resp.response,
            finish_reason: if gen_resp.done { "stop".into() } else { "length".into() },
            prompt_tokens: gen_resp.prompt_eval_count.map(|c| c as usize).unwrap_or(0),
            generated_tokens: gen_resp.eval_count.map(|c| c as usize).unwrap_or(0),
            total_tokens: 0,
            elapsed_ms: elapsed,
            correlation_id: request.correlation_id,
            tool_calls: Vec::new(),
            structured_output: None,
        })
    }

    async fn stream(
        &self,
        request: &InferenceRequest,
        sink: mpsc::Sender<InferenceResponse>,
    ) -> Result<(), ProviderError> {
        let body = OllamaGenerateReq {
            model: request.model_name.clone(),
            prompt: request.prompt.clone(),
            stream: true,
        };

        let resp = self
            .client
            .post(self.url("/api/generate"))
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Internal(format!("Ollama stream: {e}")))?;

        let mut stream = resp.bytes_stream();

        while let Some(chunk) = tokio_stream::StreamExt::next(&mut stream).await {
            let chunk = chunk.map_err(|e| ProviderError::Internal(format!("Stream chunk: {e}")))?;
            if let Ok(gen_chunk) = serde_json::from_slice::<OllamaGenerateResp>(&chunk) {
                let _ = sink
                    .send(InferenceResponse {
                        id: request.correlation_id.map_or("".into(), |c| c.to_string()),
                        model_id: request.model_id,
                        text: gen_chunk.response,
                        finish_reason: if gen_chunk.done { "stop".into() } else { String::new() },
                        prompt_tokens: 0,
                        generated_tokens: 1,
                        total_tokens: 0,
                        elapsed_ms: 0.0,
                        correlation_id: request.correlation_id,
                        tool_calls: Vec::new(),
                        structured_output: None,
                    })
                    .await;
                if gen_chunk.done {
                    break;
                }
            }
        }
        Ok(())
    }

    async fn cancel(&self, _cid: &str) -> Result<(), ProviderError> {
        Ok(())
    }
    async fn health_check(&self) -> Result<(), ProviderError> {
        self.client
            .get(self.url("/api/tags"))
            .send()
            .await
            .map_err(|e| ProviderError::Internal(format!("Ollama health: {e}")))?;
        Ok(())
    }

    async fn list_devices(&self) -> Result<Vec<DeviceInfo>, ProviderError> {
        Ok(vec![DeviceInfo {
            device_id: 0,
            name: "Ollama (HTTP)".into(),
            backend: Backend::Cpu,
            memory_bytes: 0,
            available: true,
            recommended: true,
        }])
    }

    async fn count_tokens(&self, _mid: &ModelId, text: &str) -> Result<usize, ProviderError> {
        Ok(text.len() / 3)
    }
}

// ── LM Studio provider ────────────────────────────────────────────

pub struct LmStudioProvider {
    pub base_url: String,
    client: Client,
}

impl std::fmt::Debug for LmStudioProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LmStudioProvider").field("base_url", &self.base_url).finish()
    }
}

impl LmStudioProvider {
    #[must_use]
    pub fn new(base_url: impl Into<String>) -> Self {
        Self { base_url: base_url.into(), client: build_client().expect("reqwest client") }
    }
}

#[async_trait]
impl ModelProvider for LmStudioProvider {
    fn name(&self) -> &'static str {
        "lmstudio"
    }
    fn provider_capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            text_generation: true,
            chat: true,
            tool_calling: true,
            structured_output: true,
            embeddings: true,
            ..Default::default()
        }
    }

    async fn discover_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let resp = self
            .client
            .get(&format!("{}/v1/models", self.base_url))
            .send()
            .await
            .map_err(|e| ProviderError::Internal(format!("LM Studio: {e}")))?;
        let body: OpenAiModelsResp =
            resp.json().await.map_err(|e| ProviderError::Internal(format!("Parse: {e}")))?;
        Ok(body
            .data
            .into_iter()
            .map(|m| ModelInfo {
                id: ModelId::new(),
                provider: "lmstudio".into(),
                name: m.id,
                ..ModelInfo::default()
            })
            .collect())
    }

    async fn inspect(&self, _id: &ModelId) -> Result<ModelInfo, ProviderError> {
        Err(ProviderError::Unsupported("LM Studio inspect not available".into()))
    }

    async fn load(&self, _id: &ModelId) -> Result<ModelInfo, ProviderError> {
        Ok(ModelInfo {
            id: ModelId::new(),
            provider: "lmstudio".into(),
            name: "lmstudio-model".into(),
            capabilities: self.provider_capabilities(),
            ..ModelInfo::default()
        })
    }
    async fn unload(&self, _id: &ModelId) -> Result<(), ProviderError> {
        Ok(())
    }

    async fn generate(
        &self,
        request: &InferenceRequest,
    ) -> Result<InferenceResponse, ProviderError> {
        openai_completions(&self.client, &self.base_url, request).await
    }

    async fn stream(
        &self,
        request: &InferenceRequest,
        sink: mpsc::Sender<InferenceResponse>,
    ) -> Result<(), ProviderError> {
        openai_stream(&self.client, &self.base_url, request, sink).await
    }

    async fn cancel(&self, _cid: &str) -> Result<(), ProviderError> {
        Ok(())
    }
    async fn health_check(&self) -> Result<(), ProviderError> {
        self.client
            .get(&format!("{}/v1/models", self.base_url))
            .send()
            .await
            .map_err(|_| ProviderError::Internal("LM Studio health fail".into()))?;
        Ok(())
    }
    async fn list_devices(&self) -> Result<Vec<DeviceInfo>, ProviderError> {
        Ok(vec![DeviceInfo {
            device_id: 0,
            name: "LM Studio (HTTP)".into(),
            backend: Backend::Cpu,
            memory_bytes: 0,
            available: true,
            recommended: true,
        }])
    }
    async fn count_tokens(&self, _mid: &ModelId, text: &str) -> Result<usize, ProviderError> {
        Ok(text.len() / 3)
    }
}

// ── OpenAI-compatible provider ────────────────────────────────────

pub struct OpenAiProvider {
    pub base_url: String,
    pub api_key: Option<String>,
    client: Client,
}

impl std::fmt::Debug for OpenAiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiProvider")
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "***"))
            .finish()
    }
}

impl OpenAiProvider {
    #[must_use]
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: None,
            client: build_client().expect("reqwest client"),
        }
    }
    #[must_use]
    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }
}

#[async_trait]
impl ModelProvider for OpenAiProvider {
    fn name(&self) -> &'static str {
        "openai"
    }
    fn provider_capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            text_generation: true,
            chat: true,
            tool_calling: true,
            structured_output: true,
            embeddings: true,
            reranking: true,
            vision: true,
            audio: true,
            reasoning: true,
            ..Default::default()
        }
    }

    async fn discover_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let resp = self
            .client
            .get(&format!("{}/v1/models", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key.as_deref().unwrap_or("")))
            .send()
            .await
            .map_err(|e| ProviderError::Internal(format!("OpenAI: {e}")))?;
        let body: OpenAiModelsResp =
            resp.json().await.map_err(|e| ProviderError::Internal(format!("Parse: {e}")))?;
        Ok(body
            .data
            .into_iter()
            .map(|m| ModelInfo {
                id: ModelId::new(),
                provider: "openai".into(),
                name: m.id,
                ..ModelInfo::default()
            })
            .collect())
    }

    async fn inspect(&self, _id: &ModelId) -> Result<ModelInfo, ProviderError> {
        Err(ProviderError::Unsupported("inspect not available".into()))
    }
    async fn load(&self, _id: &ModelId) -> Result<ModelInfo, ProviderError> {
        Ok(ModelInfo {
            id: ModelId::new(),
            provider: "openai".into(),
            name: "openai-model".into(),
            capabilities: self.provider_capabilities(),
            ..ModelInfo::default()
        })
    }
    async fn unload(&self, _id: &ModelId) -> Result<(), ProviderError> {
        Ok(())
    }

    async fn generate(
        &self,
        request: &InferenceRequest,
    ) -> Result<InferenceResponse, ProviderError> {
        openai_completions(&self.client, &self.base_url, request).await
    }

    async fn stream(
        &self,
        request: &InferenceRequest,
        sink: mpsc::Sender<InferenceResponse>,
    ) -> Result<(), ProviderError> {
        openai_stream(&self.client, &self.base_url, request, sink).await
    }

    async fn cancel(&self, _cid: &str) -> Result<(), ProviderError> {
        Ok(())
    }
    async fn health_check(&self) -> Result<(), ProviderError> {
        Ok(())
    }
    async fn list_devices(&self) -> Result<Vec<DeviceInfo>, ProviderError> {
        Ok(vec![DeviceInfo {
            device_id: 0,
            name: "OpenAI (HTTP)".into(),
            backend: Backend::Other,
            memory_bytes: 0,
            available: true,
            recommended: true,
        }])
    }
    async fn count_tokens(&self, _mid: &ModelId, text: &str) -> Result<usize, ProviderError> {
        Ok(text.len() / 3)
    }
}

// ── Shared OpenAI-compatible transport ────────────────────────────

async fn openai_completions(
    client: &Client,
    base_url: &str,
    request: &InferenceRequest,
) -> Result<InferenceResponse, ProviderError> {
    let start = std::time::Instant::now();
    let body = serde_json::json!({
        "model": &request.model_name,
        "messages": [
            {"role": "user", "content": &request.prompt}
        ],
        "max_tokens": request.max_tokens,
        "temperature": request.temperature,
        "stream": false
    });

    let resp = client
        .post(&format!("{base_url}/v1/chat/completions"))
        .json(&body)
        .send()
        .await
        .map_err(|e| ProviderError::Internal(format!("OpenAI compat: {e}")))?;

    let chat: OpenAiChatResp =
        resp.json().await.map_err(|e| ProviderError::Internal(format!("Parse chat: {e}")))?;

    let usage = chat.usage.clone();
    let prompt_tokens = usage.as_ref().map(|u| u.prompt_tokens as usize).unwrap_or(0);
    let generated = usage.as_ref().map(|u| u.completion_tokens as usize).unwrap_or(0);

    let choice = chat.choices.into_iter().next().unwrap_or(OpenAiChoice {
        message: OpenAiMessage { content: None, reasoning_content: None },
    });
    Ok(InferenceResponse {
        id: request.correlation_id.map_or("".into(), |c| c.to_string()),
        model_id: request.model_id,
        text: extract_usable_content(&choice.message),
        finish_reason: "stop".into(),
        prompt_tokens,
        generated_tokens: generated,
        total_tokens: 0,
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
        correlation_id: request.correlation_id,
        tool_calls: Vec::new(),
        structured_output: None,
    })
}

async fn openai_stream(
    client: &Client,
    base_url: &str,
    request: &InferenceRequest,
    sink: mpsc::Sender<InferenceResponse>,
) -> Result<(), ProviderError> {
    let body = serde_json::json!({
        "model": &request.model_name,
        "messages": [{"role": "user", "content": &request.prompt}],
        "max_tokens": request.max_tokens,
        "temperature": request.temperature,
        "stream": true
    });

    let resp = client
        .post(&format!("{base_url}/v1/chat/completions"))
        .json(&body)
        .send()
        .await
        .map_err(|e| ProviderError::Internal(format!("OpenAI stream: {e}")))?;

    let mut stream = resp.bytes_stream();
    while let Some(chunk) = tokio_stream::StreamExt::next(&mut stream).await {
        let chunk = chunk.map_err(|e| ProviderError::Internal(format!("Stream: {e}")))?;
        let text = String::from_utf8_lossy(&chunk);
        for line in text.lines() {
            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    return Ok(());
                }
                if let Ok(ev) = serde_json::from_slice::<OpenAiStreamEvent>(data.as_bytes()) {
                    if let Some(choice) = ev.choices.into_iter().next() {
                        if let Some(delta) = choice.delta {
                            let _ = sink
                                .send(InferenceResponse {
                                    id: request.correlation_id.map_or("".into(), |c| c.to_string()),
                                    model_id: request.model_id,
                                    text: delta.content.unwrap_or_default(),
                                    finish_reason: choice.finish_reason.unwrap_or_default(),
                                    prompt_tokens: 0,
                                    generated_tokens: 1,
                                    total_tokens: 0,
                                    elapsed_ms: 0.0,
                                    correlation_id: request.correlation_id,
                                    tool_calls: Vec::new(),
                                    structured_output: None,
                                })
                                .await;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

// ── Content extraction ────────────────────────────────────────────

/// Extract usable text from an OpenAI-compatible message.
///
/// If `content` is non-empty, return it. Otherwise fall back to
/// `reasoning_content` (some reasoning models like MTP variants put
/// the final answer inside reasoning). Prefixed with `[Reasoning]` to
/// distinguish it from direct content.
fn extract_usable_content(msg: &OpenAiMessage) -> String {
    if let Some(ref content) = msg.content {
        let trimmed = content.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    if let Some(ref reasoning) = msg.reasoning_content {
        let trimmed = reasoning.trim();
        if !trimmed.is_empty() {
            return format!("[Reasoning] {trimmed}");
        }
    }

    String::new()
}

// ── JSON types ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct OllamaTagsResp {
    models: Option<Vec<OllamaModel>>,
}
#[derive(Deserialize)]
struct OllamaModel {
    name: String,
}
#[derive(Serialize)]
struct OllamaGenerateReq {
    model: String,
    prompt: String,
    stream: bool,
}
#[derive(Deserialize)]
struct OllamaGenerateResp {
    response: String,
    done: bool,
    prompt_eval_count: Option<u64>,
    eval_count: Option<u64>,
}
#[derive(Deserialize)]
struct OpenAiModelsResp {
    data: Vec<OpenAiModelEntry>,
}
#[derive(Deserialize)]
struct OpenAiModelEntry {
    id: String,
}
#[derive(Deserialize)]
struct OpenAiChatResp {
    choices: Vec<OpenAiChoice>,
    usage: Option<OpenAiUsage>,
}
#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}
#[derive(Deserialize)]
struct OpenAiMessage {
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
}
#[derive(Deserialize, Clone)]
struct OpenAiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}
#[derive(Deserialize)]
struct OpenAiStreamEvent {
    choices: Vec<OpenAiStreamChoice>,
}
#[derive(Deserialize)]
struct OpenAiStreamChoice {
    delta: Option<OpenAiStreamDelta>,
    finish_reason: Option<String>,
}
#[derive(Deserialize)]
struct OpenAiStreamDelta {
    content: Option<String>,
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn ollama_health_check() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"models":[]})),
            )
            .mount(&server)
            .await;
        let provider = OllamaProvider::new(server.uri());
        assert!(provider.health_check().await.is_ok());
    }

    #[tokio::test]
    async fn ollama_generate_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/generate"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"response":"Hello world","done":true,"eval_count":3}),
            ))
            .mount(&server)
            .await;
        let provider = OllamaProvider::new(server.uri());
        let result = provider
            .generate(&InferenceRequest {
                model_id: ModelId::new(),
                model_name: "test-model".into(),
                prompt: "hi".into(),
                max_tokens: 100,
                temperature: 0.7,
                top_p: None,
                top_k: None,
                seed: None,
                stop_sequences: Vec::new(),
                grammar: None,
                tools: Vec::new(),
                system_prompt: None,
                correlation_id: None,
                task_id: None,
                timeout_ms: None,
                context_budget: crate::inference::ContextBudget::new(4096),
            })
            .await
            .unwrap();
        assert_eq!(result.text, "Hello world");
        assert_eq!(result.generated_tokens, 3);
    }

    #[tokio::test]
    async fn ollama_error_500() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/generate"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let provider = OllamaProvider::new(server.uri());
        assert!(
            provider
                .generate(&InferenceRequest {
                    model_id: ModelId::new(),
                    model_name: "test-model".into(),
                    prompt: "x".into(),
                    max_tokens: 1,
                    temperature: 0.0,
                    top_p: None,
                    top_k: None,
                    seed: None,
                    stop_sequences: Vec::new(),
                    grammar: None,
                    tools: Vec::new(),
                    system_prompt: None,
                    correlation_id: None,
                    task_id: None,
                    timeout_ms: None,
                    context_budget: crate::inference::ContextBudget::new(4096),
                })
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn lmstudio_discover() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"object":"list","data":[{"id":"test-model","object":"model"}]}),
            ))
            .mount(&server)
            .await;
        let provider = LmStudioProvider::new(server.uri());
        let models = provider.discover_models().await.unwrap();
        assert_eq!(models.len(), 1);
    }

    #[tokio::test]
    async fn openai_chat_success() {
        let server = MockServer::start().await;
        let body = serde_json::json!({
            "choices": [{"message": {"content": "Hi!"}}],
            "usage": {"prompt_tokens": 5, "completion_tokens": 2, "total_tokens": 7}
        });
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        let provider = OpenAiProvider::new(server.uri());
        let req = InferenceRequest {
            model_id: ModelId::new(),
            model_name: "test-model".into(),
            prompt: "hello".into(),
            max_tokens: 10,
            temperature: 0.5,
            top_p: None,
            top_k: None,
            seed: None,
            stop_sequences: Vec::new(),
            grammar: None,
            tools: Vec::new(),
            system_prompt: None,
            correlation_id: None,
            task_id: None,
            timeout_ms: None,
            context_budget: crate::inference::ContextBudget::new(4096),
        };
        let result = provider.generate(&req).await.unwrap();
        assert_eq!(result.text, "Hi!");
        assert_eq!(result.prompt_tokens, 5);
    }
}
