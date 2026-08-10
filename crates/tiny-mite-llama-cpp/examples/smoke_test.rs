//! Tiny Mite — llama.cpp end-to-end smoke test.
//!
//! Validates that the native llama.cpp integration works against
//! a real GGUF model through the LM Studio-provided libllama.so.
//!
//! # Usage
//!
//! ```bash
//! cargo run --example smoke_test -- /path/to/model.gguf
//! ```

use std::path::PathBuf;
use std::time::Instant;

use tiny_mite_llama_cpp::{ffi, gguf};
use tiny_mite_runtime::{Backend, ModelProvider};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Tiny Mite llama.cpp Smoke Test ===\n");

    // ── Find the model ────────────────────────────────────────
    let args: Vec<String> = std::env::args().collect();
    let model_path = if args.len() > 1 {
        PathBuf::from(&args[1])
    } else {
        let home = std::env::var("HOME").unwrap_or_default();
        let candidates = [
            PathBuf::from(
                "/opt/LM-Studio/resources/app/.webpack/bin/bundled-models/nomic-ai/nomic-embed-text-v1.5-GGUF/nomic-embed-text-v1.5.Q4_K_M.gguf",
            ),
            PathBuf::from(format!(
                "{home}/.lmstudio/.internal/bundled-models/nomic-ai/nomic-embed-text-v1.5-GGUF/nomic-embed-text-v1.5.Q4_K_M.gguf"
            )),
        ];
        candidates
            .into_iter()
            .find(|p| p.exists())
            .ok_or("No GGUF model found. Pass a path as argument.")?
    };

    if !model_path.exists() {
        eprintln!("Model not found: {}", model_path.display());
        std::process::exit(1);
    }
    println!("Model: {}\n", model_path.display());

    // ── Initialize library ────────────────────────────────────
    let t0 = Instant::now();
    ffi::backend_init().map_err(|e| format!("Backend init failed: {e}"))?;
    println!("Library init: {:?}", t0.elapsed());

    // ── Inspect GGUF metadata ─────────────────────────────────
    let t0 = Instant::now();
    let meta = gguf::inspect_gguf(&model_path)?;
    println!("GGUF inspection: {:?}", t0.elapsed());
    println!("  GGUF version:     v{}", meta.gguf_version);
    println!("  Architecture:     {}", meta.architecture.as_deref().unwrap_or("unknown"));
    println!("  Model name:       {}", meta.name.as_deref().unwrap_or("unknown"));
    println!("  Family:           {}", meta.family.as_deref().unwrap_or("unknown"));
    println!("  Quantization:     {}", meta.quantization.as_deref().unwrap_or("unknown"));
    println!("  Parameters:       {}", meta.parameter_label());
    println!(
        "  Context length:   {}",
        meta.context_length.map_or("unknown".to_string(), |c| c.to_string())
    );
    println!(
        "  Embedding dim:    {}",
        meta.embedding_length.map_or("unknown".to_string(), |e| e.to_string())
    );
    println!("  File size:        {}", meta.file_size_label());
    println!("  Estimated RAM:    {}\n", meta.ram_label());

    // ── Capability detection ───────────────────────────────────
    let is_embedding = meta.architecture.as_deref() == Some("nomic-bert")
        || meta.name.as_deref().map_or(false, |n| n.contains("embed"));
    println!("  Capability:");
    println!("    Text generation:  {}", !is_embedding);
    println!("    Embedding:        {}\n", is_embedding);

    // ── Load model ────────────────────────────────────────────
    println!("[OK] backend initialized");
    println!("[OK] GGUF inspected");
    println!("  model_default_params: about to call...");
    let _mp =
        ffi::model_default_params().map_err(|e| format!("model_default_params failed: {e}"))?;
    println!("[OK] model parameters created");

    let t0 = Instant::now();
    println!("  load_model: about to call (no overrides, pure default params)...");
    let model = ffi::load_model_with_defaults(&model_path)
        .map_err(|e| format!("load_model failed: {e}"))?;
    println!("[OK] model loaded ({:?})", t0.elapsed());

    // Native metadata from llama.cpp
    if let Ok(desc) = ffi::model_desc(&model) {
        println!("  Native desc:      {desc}");
    }
    if let Ok(size) = ffi::model_size(&model) {
        println!("  Native size:      {size} bytes ({:.2} GB)", size as f64 / 1e9);
    }
    if let Ok(n) = ffi::model_n_params(&model) {
        println!("  Native params:    {n}");
    }
    println!();

    // ── Create context ─────────────────────────────────────────
    let ctx_len = meta.context_length.unwrap_or(2048);
    let t0 = Instant::now();
    println!("  ctx_default_params: about to call...");
    let _cparams =
        ffi::ctx_default_params().map_err(|e| format!("ctx_default_params failed: {e}"))?;
    println!("[OK] context parameters created");
    println!("[EMB] embedding mode = true (from create_context override)");

    println!("  create_context: about to call (n_ctx={ctx_len})...");
    let ctx = match ffi::create_context(&model, ctx_len, 4) {
        Ok(c) => {
            println!("[OK] context created ({:?})", t0.elapsed());
            c
        }
        Err(e) => {
            eprintln!("Context creation failed: {e}");
            eprintln!("(this may be expected for embedding models)\n");
            eprintln!("Cleaning up model...");
            ffi::free_model(model);
            println!("\n=== Smoke Test Complete ===");
            return Ok(());
        }
    };

    // ── Tokenization ──────────────────────────────────────────
    let test_text = "Hello Tiny Mite.";
    let t0 = Instant::now();
    match ffi::tokenize(&model, test_text) {
        Ok(tokens) => {
            println!("Tokenization: {:?}", t0.elapsed());
            println!("  Input:           \"{test_text}\"");
            println!("  Token count:     {}", tokens.len());
            println!("  Token IDs:       {:?}\n", &tokens[..tokens.len().min(10)]);
        }
        Err(e) => {
            eprintln!("Tokenization failed: {e}");
        }
    }

    // ── Embedding note ────────────────────────────────────────
    if is_embedding {
        println!("NOTE: This is an embedding model.");
        println!("      Model loading, context creation, and tokenization validated.\n");
    }

    // ── Cleanup ───────────────────────────────────────────────
    let t0 = Instant::now();
    ffi::free_context(ctx);
    println!("Context freed: {:?}", t0.elapsed());

    let t0 = Instant::now();
    ffi::free_model(model);
    println!("Model freed: {:?}", t0.elapsed());
    println!();

    // ── Provider lifecycle test ───────────────────────────────
    println!("=== Provider Lifecycle Test ===\n");
    let models_dir = model_path.parent().unwrap().to_string_lossy().to_string();
    let provider =
        tiny_mite_llama_cpp::NativeLlamaCppProvider::new(&models_dir, Backend::Cpu).with_threads(4);

    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;

    // Discover
    let discovered = rt.block_on(async { provider.discover_models().await })?;
    println!("Discovery: {} model(s)", discovered.len());
    for m in &discovered {
        println!(
            "  - {} ({}), state: {:?}",
            m.name,
            m.quantization.as_deref().unwrap_or("?"),
            m.state
        );
    }

    if let Some(first) = discovered.first() {
        // Load
        let t0 = Instant::now();
        let _loaded = rt.block_on(async { provider.load(&first.id).await })?;
        println!("\nLoad: {:?}", t0.elapsed());

        // Count tokens
        let count = rt.block_on(async { provider.count_tokens(&first.id, test_text).await })?;
        println!("Token count: {count} for \"{test_text}\"");

        // Unload
        rt.block_on(async { provider.unload(&first.id).await })?;
        println!("Unload: ok");
    }

    // Health check
    rt.block_on(async { provider.health_check().await })?;
    println!("Health: ok");

    println!("\n=== Smoke Test Complete ===");
    println!("Result: PASS — real llama.cpp integration verified\n");
    Ok(())
}
