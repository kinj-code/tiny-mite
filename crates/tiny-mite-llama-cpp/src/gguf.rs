//! GGUF file parser for model metadata extraction.
//!
//! Reads ONLY the header and metadata sections, never touching tensor payload data.
//! Supports GGUF v2 and v3.
//!
//! # Safety
//! - All model files are untrusted input
//! - Integer overflow protection on all allocations
//! - Metadata counts capped at 100k
//! - No unsafe code

use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::path::Path;

// ── Error ────────────────────────────────────────────────────────

/// Errors that can occur during GGUF inspection.
#[derive(Debug, thiserror::Error)]
pub enum GgufError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("Invalid GGUF magic bytes")]
    InvalidMagic,
    #[error("Unsupported GGUF version {found}; supported: {supported:?}")]
    UnsupportedVersion { found: u32, supported: Vec<u32> },
    #[error("File too small ({actual} bytes, need at least {expected})")]
    TruncatedFile { expected: u64, actual: u64 },
    #[error("Excessive metadata/tensor count: {0}")]
    ExcessiveCount(u64),
    #[error("String exceeds maximum length: {0} bytes")]
    StringTooLong(usize),
    #[error("Not a regular file: {0}")]
    NotRegularFile(String),
}

// ── GGUF Value ───────────────────────────────────────────────────

/// A single metadata value from a GGUF file.
#[derive(Debug, Clone, PartialEq)]
pub enum GgufValue {
    U8(u8),
    I8(i8),
    U16(u16),
    I16(i16),
    U32(u32),
    I32(i32),
    U64(u64),
    I64(i64),
    F32(f32),
    F64(f64),
    Bool(bool),
    String(String),
    Array(Vec<GgufValue>),
}

impl GgufValue {
    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Self::U32(v) => Some(*v),
            _ => None,
        }
    }
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::U64(v) => Some(*v),
            _ => None,
        }
    }
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Self::F32(v) => Some(*v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s.as_str()),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

// ── GGUF dtype ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GgufDtype {
    F32,
    F16,
    Q4_0,
    Q4_1,
    Q5_0,
    Q5_1,
    Q8_0,
    Q8_1,
    Q2K,
    Q3K,
    Q4K,
    Q5K,
    Q6K,
    Q8K,
    Iq2Xxs,
    Iq2Xs,
    Iq3Xxs,
    Iq3S,
    Iq2S,
    Iq1S,
    Iq4Nl,
    Iq4Xs,
    Bf16,
    I32,
    I8,
    Other(u32),
}

impl GgufDtype {
    pub fn element_size(&self) -> f64 {
        match self {
            Self::F32 | Self::I32 => 4.0,
            Self::F16 | Self::Bf16 => 2.0,
            Self::Q4_0 | Self::Q4_1 => 0.5,
            Self::Q5_0 | Self::Q5_1 => 0.625,
            Self::Q8_0 | Self::Q8_1 | Self::I8 => 1.0,
            Self::Q2K => 0.3125,
            Self::Q3K => 0.4375,
            Self::Q4K => 0.5,
            Self::Q5K => 0.625,
            Self::Q6K => 0.75,
            Self::Q8K => 1.0,
            Self::Iq2Xxs | Self::Iq2Xs | Self::Iq2S => 0.3125,
            Self::Iq3Xxs | Self::Iq3S => 0.4375,
            Self::Iq1S => 0.1875,
            Self::Iq4Nl | Self::Iq4Xs => 0.5,
            Self::Other(_) => 0.5,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::F32 => "F32",
            Self::F16 => "F16",
            Self::Q4_0 => "Q4_0",
            Self::Q4_1 => "Q4_1",
            Self::Q5_0 => "Q5_0",
            Self::Q5_1 => "Q5_1",
            Self::Q8_0 => "Q8_0",
            Self::Q8_1 => "Q8_1",
            Self::Q2K => "Q2_K",
            Self::Q3K => "Q3_K",
            Self::Q4K => "Q4_K",
            Self::Q5K => "Q5_K",
            Self::Q6K => "Q6_K",
            Self::Q8K => "Q8_K",
            Self::Iq2Xxs => "IQ2_XXS",
            Self::Iq2Xs => "IQ2_XS",
            Self::Iq3Xxs => "IQ3_XXS",
            Self::Iq3S => "IQ3_S",
            Self::Iq2S => "IQ2_S",
            Self::Iq1S => "IQ1_S",
            Self::Iq4Nl => "IQ4_NL",
            Self::Iq4Xs => "IQ4_XS",
            Self::Bf16 => "BF16",
            Self::I32 => "I32",
            Self::I8 => "I8",
            Self::Other(_) => "unknown",
        }
    }
}

// ── Confidence ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    High,
    Medium,
    Low,
}

// ── ModelMetadata ─────────────────────────────────────────────────

/// Extracted model metadata from a GGUF file.
#[derive(Debug, Clone)]
pub struct ModelMetadata {
    pub gguf_version: u32,
    pub architecture: Option<String>,
    pub name: Option<String>,
    pub family: Option<String>,
    pub quantization: Option<String>,
    pub quantization_source: Confidence,
    pub parameter_count: Option<u64>,
    pub parameter_source: Confidence,
    pub context_length: Option<u32>,
    pub embedding_length: Option<u32>,
    pub block_count: Option<u32>,
    pub attention_heads: Option<u32>,
    pub kv_heads: Option<u32>,
    pub vocab_size: Option<u32>,
    pub tensor_count: u64,
    pub file_size: u64,
    pub estimated_ram_bytes: u64,
    pub split_count: Option<u32>,
    pub split_index: Option<u32>,
    pub raw_metadata: HashMap<String, GgufValue>,
}

impl ModelMetadata {
    pub fn parameter_label(&self) -> String {
        match self.parameter_count {
            Some(p) if p >= 1_000_000_000 => format!("{:.1}B", p as f64 / 1e9),
            Some(p) => format!("{}M", p / 1_000_000),
            None => "unknown".into(),
        }
    }
    pub fn file_size_label(&self) -> String {
        let gb = self.file_size as f64 / 1_073_741_824.0;
        if gb >= 1.0 { format!("{gb:.2} GB") } else { format!("{} MB", self.file_size / 1_048_576) }
    }
    pub fn ram_label(&self) -> String {
        let gb = self.estimated_ram_bytes as f64 / 1_073_741_824.0;
        format!("~{gb:.1} GB")
    }
}

// ── Parser ───────────────────────────────────────────────────────

pub fn inspect_gguf(path: &Path) -> Result<ModelMetadata, GgufError> {
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        return Err(GgufError::NotRegularFile(path.display().to_string()));
    }
    let file_size = meta.len();
    if file_size < 32 {
        return Err(GgufError::TruncatedFile { expected: 32, actual: file_size });
    }

    let mut file = fs::File::open(path)?;
    let mut r = io::BufReader::new(&mut file);

    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)?;
    if &magic != b"GGUF" {
        return Err(GgufError::InvalidMagic);
    }

    let version = read_u32le(&mut r)?;
    let supported = vec![2u32, 3];
    if !supported.contains(&version) {
        return Err(GgufError::UnsupportedVersion { found: version, supported });
    }

    let tensor_count = read_u64le(&mut r)?;
    let kv_count = read_u64le(&mut r)?;
    const MAX: u64 = 100_000;
    if tensor_count > MAX || kv_count > MAX {
        return Err(GgufError::ExcessiveCount(tensor_count.max(kv_count)));
    }

    let mut raw_meta = HashMap::with_capacity(kv_count.min(10_000) as usize);
    for _ in 0..kv_count {
        let (k, v) = read_kv(&mut r)?;
        raw_meta.insert(k, v);
    }

    let mut total_elements: u64 = 0;
    let mut dominant_dtype: Option<GgufDtype> = None;
    for _ in 0..tensor_count {
        let (name, dims, dtype, _off) = read_tensor_info(&mut r)?;
        let elements: u64 = dims.iter().map(|&d| d as u64).product();
        if !name.contains("token_embd") && !name.contains("output") {
            total_elements = total_elements.saturating_add(elements);
        }
        if dominant_dtype.is_none() && dtype != GgufDtype::F32 {
            dominant_dtype = Some(dtype);
        }
    }

    let arch = raw_meta.get("general.architecture").and_then(|v| v.as_str()).map(String::from);
    let name = raw_meta.get("general.name").and_then(|v| v.as_str()).map(String::from);
    let family = arch
        .as_ref()
        .map(|a| a.split(|c: char| c == '-' || c == '_').next().unwrap_or(a).to_lowercase());

    let prefix = arch.as_deref().unwrap_or("general");
    let ctx = raw_meta
        .get(&format!("{prefix}.context_length"))
        .and_then(|v| v.as_u32())
        .or_else(|| raw_meta.get("general.context_length").and_then(|v| v.as_u32()));
    let blocks = raw_meta
        .get(&format!("{prefix}.block_count"))
        .and_then(|v| v.as_u32())
        .or_else(|| raw_meta.get("general.block_count").and_then(|v| v.as_u32()));
    let embd = raw_meta.get(&format!("{prefix}.embedding_length")).and_then(|v| v.as_u32());
    let heads = raw_meta.get(&format!("{prefix}.attention.head_count")).and_then(|v| v.as_u32());
    let kv_h = raw_meta.get(&format!("{prefix}.attention.head_count_kv")).and_then(|v| v.as_u32());
    let vocab = raw_meta.get(&format!("{prefix}.vocab_size")).and_then(|v| v.as_u32());

    let (quant, qsrc) = if let Some(dt) = dominant_dtype {
        (Some(dt.label().into()), Confidence::High)
    } else {
        (None, Confidence::Low)
    };

    let (params, psrc) =
        if let Some(v) = raw_meta.get("general.parameter_count").and_then(|v| v.as_u64()) {
            (Some(v), Confidence::High)
        } else if total_elements > 0 {
            (Some(total_elements), Confidence::Medium)
        } else {
            (None, Confidence::Low)
        };

    let el_size = dominant_dtype.map(|d| d.element_size()).unwrap_or(2.0);
    let weight_bytes = (total_elements as f64 * el_size) as u64;
    let overhead = 256 * 1024 * 1024;
    let kv_cache = estimate_kv_cache(ctx, embd, blocks, kv_h);
    let ram = weight_bytes.saturating_add(overhead).saturating_add(kv_cache);

    let (sc, si) = detect_split(path);

    Ok(ModelMetadata {
        gguf_version: version,
        architecture: arch,
        name,
        family,
        quantization: quant,
        quantization_source: qsrc,
        parameter_count: params,
        parameter_source: psrc,
        context_length: ctx,
        embedding_length: embd,
        block_count: blocks,
        attention_heads: heads,
        kv_heads: kv_h,
        vocab_size: vocab,
        tensor_count,
        file_size,
        estimated_ram_bytes: ram,
        split_count: sc,
        split_index: si,
        raw_metadata: raw_meta,
    })
}

// ── Readers ──────────────────────────────────────────────────────

fn read_u32le<R: Read>(r: &mut R) -> io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}
fn read_u64le<R: Read>(r: &mut R) -> io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}
fn read_i32le<R: Read>(r: &mut R) -> io::Result<i32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(i32::from_le_bytes(b))
}
fn read_f32le<R: Read>(r: &mut R) -> io::Result<f32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(f32::from_le_bytes(b))
}

fn read_string<R: Read>(r: &mut R) -> io::Result<String> {
    let len = read_u64le(r)? as usize;
    if len > 10_000_000 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "string too long"));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn read_dtype<R: Read>(r: &mut R) -> io::Result<GgufDtype> {
    Ok(match read_u32le(r)? {
        0 => GgufDtype::F32,
        1 => GgufDtype::F16,
        2 => GgufDtype::Q4_0,
        3 => GgufDtype::Q4_1,
        6 => GgufDtype::Q5_0,
        7 => GgufDtype::Q5_1,
        8 => GgufDtype::Q8_0,
        9 => GgufDtype::Q8_1,
        10 => GgufDtype::Q2K,
        11 => GgufDtype::Q3K,
        12 => GgufDtype::Q4K,
        13 => GgufDtype::Q5K,
        14 => GgufDtype::Q6K,
        15 => GgufDtype::Q8K,
        16 => GgufDtype::Iq2Xxs,
        17 => GgufDtype::Iq2Xs,
        18 => GgufDtype::Iq3Xxs,
        19 => GgufDtype::Iq3S,
        20 => GgufDtype::Iq2S,
        21 => GgufDtype::Iq1S,
        22 => GgufDtype::Iq4Nl,
        23 => GgufDtype::Iq4Xs,
        30 => GgufDtype::Bf16,
        24 => GgufDtype::I32,
        25 => GgufDtype::I8,
        o => GgufDtype::Other(o),
    })
}

fn read_value<R: Read>(r: &mut R, gtype: u32) -> io::Result<GgufValue> {
    Ok(match gtype {
        0 => GgufValue::U8({
            let mut b = [0u8; 1];
            r.read_exact(&mut b)?;
            b[0]
        }),
        1 => GgufValue::I8({
            let mut b = [0u8; 1];
            r.read_exact(&mut b)?;
            b[0] as i8
        }),
        2 => GgufValue::U16({
            let mut b = [0u8; 2];
            r.read_exact(&mut b)?;
            u16::from_le_bytes(b)
        }),
        3 => GgufValue::I16({
            let mut b = [0u8; 2];
            r.read_exact(&mut b)?;
            i16::from_le_bytes(b)
        }),
        4 => GgufValue::U32(read_u32le(r)?),
        5 => GgufValue::I32(read_i32le(r)?),
        6 => GgufValue::F32(read_f32le(r)?),
        7 => GgufValue::Bool({
            let mut b = [0u8; 1];
            r.read_exact(&mut b)?;
            b[0] != 0
        }),
        8 => GgufValue::String(read_string(r)?),
        9 => {
            let it = read_u32le(r)?;
            let n = read_u64le(r)? as usize;
            if n > 1_000_000 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "array too long"));
            }
            let mut arr = Vec::with_capacity(n);
            for _ in 0..n {
                arr.push(read_value(r, it)?);
            }
            GgufValue::Array(arr)
        }
        10 => GgufValue::U64(read_u64le(r)?),
        11 => GgufValue::I64({
            let mut b = [0u8; 8];
            r.read_exact(&mut b)?;
            i64::from_le_bytes(b)
        }),
        12 => GgufValue::F64({
            let mut b = [0u8; 8];
            r.read_exact(&mut b)?;
            f64::from_le_bytes(b)
        }),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown gguf type {gtype}"),
            ));
        }
    })
}

fn read_kv<R: Read>(r: &mut R) -> io::Result<(String, GgufValue)> {
    let key = read_string(r)?;
    if key.len() > 1024 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "key too long"));
    }
    let gtype = read_u32le(r)?;
    let val = read_value(r, gtype)?;
    Ok((key, val))
}

fn read_tensor_info<R: Read>(r: &mut R) -> io::Result<(String, Vec<u32>, GgufDtype, u64)> {
    let name = read_string(r)?;
    let ndim = read_u32le(r)? as usize;
    if ndim > 16 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "too many dims"));
    }
    let mut dims = Vec::with_capacity(ndim);
    for _ in 0..ndim {
        dims.push(read_u64le(r)? as u32);
    }
    let dtype = read_dtype(r)?;
    let offset = read_u64le(r)?;
    Ok((name, dims, dtype, offset))
}

fn detect_split(path: &Path) -> (Option<u32>, Option<u32>) {
    let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let parts: Vec<&str> = fname.split('-').collect();
    if parts.len() >= 4 {
        if let (Ok(idx), Ok(total)) = (
            parts[parts.len() - 3].parse::<u32>(),
            parts[parts.len() - 1].trim_end_matches(".gguf").parse::<u32>(),
        ) {
            return (Some(total), Some(idx.saturating_sub(1)));
        }
    }
    (None, None)
}

fn estimate_kv_cache(
    ctx: Option<u32>,
    embd: Option<u32>,
    blocks: Option<u32>,
    _kv_h: Option<u32>,
) -> u64 {
    let c = ctx.unwrap_or(2048) as u64;
    let e = embd.unwrap_or(4096) as u64;
    let l = blocks.unwrap_or(32) as u64;
    // 2 * layers * context * embedding * sizeof(F16)
    2 * l * c * e * 2
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn write_string(buf: &mut Vec<u8>, s: &str) {
        let bytes = s.as_bytes();
        buf.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        buf.extend_from_slice(bytes);
    }

    fn write_value(buf: &mut Vec<u8>, v: &GgufValue) {
        match v {
            GgufValue::U32(x) => {
                buf.extend_from_slice(&4u32.to_le_bytes());
                buf.extend_from_slice(&x.to_le_bytes());
            }
            GgufValue::U64(x) => {
                buf.extend_from_slice(&10u32.to_le_bytes());
                buf.extend_from_slice(&x.to_le_bytes());
            }
            GgufValue::String(s) => {
                buf.extend_from_slice(&8u32.to_le_bytes());
                write_string(buf, s);
            }
            GgufValue::F32(x) => {
                buf.extend_from_slice(&6u32.to_le_bytes());
                buf.extend_from_slice(&x.to_le_bytes());
            }
            GgufValue::Bool(b) => {
                buf.extend_from_slice(&7u32.to_le_bytes());
                buf.push(u8::from(*b));
            }
            _ => panic!("unsupported test value"),
        }
    }

    fn build_gguf(
        version: u32,
        meta: Vec<(&str, GgufValue)>,
        tensors: Vec<(&str, Vec<u32>, GgufDtype)>,
    ) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"GGUF");
        buf.extend_from_slice(&version.to_le_bytes());
        buf.extend_from_slice(&(tensors.len() as u64).to_le_bytes());
        buf.extend_from_slice(&(meta.len() as u64).to_le_bytes());
        for (k, v) in &meta {
            write_string(&mut buf, k);
            write_value(&mut buf, v);
        }
        for (name, dims, dtype) in &tensors {
            write_string(&mut buf, name);
            buf.extend_from_slice(&(dims.len() as u32).to_le_bytes());
            for &d in dims {
                buf.extend_from_slice(&(d as u64).to_le_bytes());
            }
            let dt: u32 = match dtype {
                GgufDtype::F32 => 0,
                GgufDtype::F16 => 1,
                _ => 0,
            };
            buf.extend_from_slice(&dt.to_le_bytes());
            buf.extend_from_slice(&0u64.to_le_bytes());
        }
        buf
    }

    #[test]
    fn valid_gguf_parses() {
        let data = build_gguf(
            3,
            vec![
                ("general.architecture", GgufValue::String("llama".into())),
                ("general.name", GgufValue::String("test-model".into())),
                ("llama.context_length", GgufValue::U32(2048)),
                ("llama.block_count", GgufValue::U32(12)),
                ("llama.embedding_length", GgufValue::U32(768)),
            ],
            vec![
                ("token_embd.weight", vec![32000, 768], GgufDtype::F32),
                ("blk.0.attn_q.weight", vec![768, 768], GgufDtype::F16),
            ],
        );
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &data).unwrap();
        let r = inspect_gguf(tmp.path()).unwrap();
        assert_eq!(r.gguf_version, 3);
        assert_eq!(r.architecture.as_deref(), Some("llama"));
        assert_eq!(r.name.as_deref(), Some("test-model"));
        assert_eq!(r.context_length, Some(2048));
    }

    #[test]
    fn invalid_magic() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &[0u8; 40]).unwrap();
        assert!(matches!(inspect_gguf(tmp.path()), Err(GgufError::InvalidMagic)));
    }

    #[test]
    fn unsupported_version() {
        // Build valid GGUF header with unsupported version, padded to minimum size
        let mut data = Vec::new();
        data.extend_from_slice(b"GGUF");
        data.extend_from_slice(&99u32.to_le_bytes());
        data.extend_from_slice(&0u64.to_le_bytes()); // tensor count
        data.extend_from_slice(&0u64.to_le_bytes()); // metadata count
        while data.len() < 32 {
            data.push(0);
        }
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &data).unwrap();
        assert!(matches!(inspect_gguf(tmp.path()), Err(GgufError::UnsupportedVersion { .. })));
    }

    #[test]
    fn truncated_file() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"GGUF").unwrap();
        assert!(inspect_gguf(tmp.path()).is_err());
    }

    #[test]
    fn split_detection() {
        let p = Path::new("/models/llama-7B-00001-of-00003.gguf");
        assert_eq!(detect_split(p), (Some(3), Some(0)));
    }

    #[test]
    fn no_split() {
        let p = Path::new("/models/llama-7B-Q4_K_M.gguf");
        assert_eq!(detect_split(p), (None, None));
    }

    #[test]
    fn parameter_label() {
        let mut md = ModelMetadata {
            gguf_version: 3,
            architecture: None,
            name: None,
            family: None,
            quantization: None,
            quantization_source: Confidence::High,
            parameter_count: Some(7_000_000_000),
            parameter_source: Confidence::High,
            context_length: None,
            embedding_length: None,
            block_count: None,
            attention_heads: None,
            kv_heads: None,
            vocab_size: None,
            tensor_count: 0,
            file_size: 0,
            estimated_ram_bytes: 0,
            split_count: None,
            split_index: None,
            raw_metadata: HashMap::new(),
        };
        assert_eq!(md.parameter_label(), "7.0B");
    }
}
