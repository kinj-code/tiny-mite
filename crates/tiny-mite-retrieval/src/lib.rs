//! Tiny Mite retrieval: embedding, hybrid search, reranking, and context pipeline.
//!
//! Provider-independent retrieval architecture. Supports lexical, vector,
//! and hybrid search with optional reranking.

#![deny(unsafe_code)]
#![warn(missing_docs)]
#![deny(unused_must_use)]

pub mod pipeline;
pub mod search;

pub use pipeline::{RetrievalCandidate, RetrievalPipeline, SearchQuery};
pub use search::{HybridRanker, LexicalSearcher};
