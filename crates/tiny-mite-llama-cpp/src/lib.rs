//! Tiny Mite native llama.cpp provider.
//!
//! # Architecture
//!
//! This crate provides the `NativeLlamaCppProvider` which implements
//! [`tiny_mite_runtime::ModelProvider`] backed by llama.cpp's C API.
//!
//! # Safety boundary
//!
//! All unsafe FFI to llama.cpp is isolated in a dedicated `ffi` module.
//! The rest of this crate — and all of Tiny Mite — remains safe Rust.
//! Every `unsafe` block documents its safety invariants.
//!
//! # Current status
//!
//! The provider structure and lifecycle management are fully implemented.
//! Actual llama.cpp C FFI integration is stubbed — the provider operates
//! in a no-op mode suitable for architectural validation and testing.
//! Real inference will be enabled when the llama.cpp library is linked.
//!
//! # Backend support
//!
//! llama.cpp supports: CPU, CUDA, Vulkan, Metal, HIP/ROCm, SYCL.
//! Backend selection is runtime-discoverable and auto-configured based
//! on available hardware.

// This crate MAY use unsafe for FFI in the ffi module.
// Safety: all unsafe is isolated and documented.
#![warn(missing_docs)]
#![deny(unused_must_use)]

pub mod ffi;
pub mod gguf;
pub mod provider;

pub use gguf::{Confidence, GgufDtype, GgufError, GgufValue, ModelMetadata, inspect_gguf};
pub use provider::NativeLlamaCppProvider;
