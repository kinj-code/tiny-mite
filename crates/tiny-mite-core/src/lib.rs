//! Tiny Mite core runtime
//!
//! Provides the foundational services that all other subsystems depend on:
//!
//! - **Configuration** — typed, layered, environment-aware config
//! - **Diagnostics** — structured tracing/logging with correlation
//! - **Error integration** — domain error type aliases and helpers
//! - **Runtime lifecycle** — coordinated startup and shutdown

#![deny(unsafe_code)]
#![warn(missing_docs)]
#![deny(unused_must_use)]

pub mod config;
pub mod diagnostics;
pub mod error;
pub mod lifecycle;
