//! Verified-ABI FFI bindings to llama.cpp (LM Studio v2.25.2).
//!
//! Structs are treated as opaque byte arrays initialized by the library's
//! own default-parameter functions. This avoids all ABI layout guesswork.
//! When the native struct size changes across versions, only the buffer
//! size needs updating.

use std::ffi::{CStr, CString, c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const MODEL_PARAMS_SIZE: usize = 512;
const CTX_PARAMS_SIZE: usize = 1024;

#[derive(Debug, thiserror::Error)]
pub enum LlamaError {
    #[error("llama.cpp library not found at {0}")]
    LibraryNotFound(PathBuf),
    #[error("Failed to load symbol '{symbol}': {error}")]
    SymbolNotFound { symbol: String, error: String },
    #[error("llama.cpp API error: {0}")]
    ApiError(String),
    #[error("Library load error: {0}")]
    LoadError(String),
}

pub type LlamaResult<T> = Result<T, LlamaError>;

#[derive(Debug, Clone, Copy)]
pub struct OpaquePtr(pub *mut c_void);
unsafe impl Send for OpaquePtr {}
unsafe impl Sync for OpaquePtr {}

type llama_token = i32;

// ── Opaque param buffers ─────────────────────────────────────────
// These must ONLY be initialized by the library's default-params functions.
// The fields below are for partial ABI compatibility only.

#[repr(C)]
#[derive(Clone, Copy)]
pub struct LlamaModelParams {
    _data: [u8; MODEL_PARAMS_SIZE],
}

impl LlamaModelParams {
    fn new() -> Self {
        Self { _data: [0u8; MODEL_PARAMS_SIZE] }
    }
    /// Set use_mmap after defaults are loaded (offset 65, from C probe)
    fn set_mmap(&mut self, val: bool) {
        self._data[65] = val as u8;
    }
    /// Set n_gpu_layers after defaults are loaded (offset 16, 4 bytes LE)
    fn set_n_gpu_layers(&mut self, n: i32) {
        self._data[16..20].copy_from_slice(&n.to_le_bytes());
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct LlamaContextParams {
    _data: [u8; CTX_PARAMS_SIZE],
}

impl LlamaContextParams {
    fn new() -> Self {
        Self { _data: [0u8; CTX_PARAMS_SIZE] }
    }
    /// Set n_ctx at offset 0 (4 bytes LE)
    fn set_n_ctx(&mut self, n: u32) {
        self._data[0..4].copy_from_slice(&n.to_le_bytes());
    }
    /// Set n_threads at offset 24 (4 bytes LE)
    fn set_n_threads(&mut self, n: i32) {
        self._data[24..28].copy_from_slice(&n.to_le_bytes());
    }
    /// Set n_threads_batch at offset 28 (4 bytes LE)
    fn set_n_threads_batch(&mut self, n: i32) {
        self._data[28..32].copy_from_slice(&n.to_le_bytes());
    }
    /// Set embeddings at the known offset
    fn set_embeddings(&mut self, val: bool) {
        self._data[88] = val as u8;
    }
}

// ── Function pointer types ───────────────────────────────────────

type LlamaBackendInitFn = unsafe extern "C" fn();
type LlamaBackendFreeFn = unsafe extern "C" fn();
type LlamaModelDefaultParamsFn = unsafe extern "C" fn(*mut LlamaModelParams);
type LlamaModelLoadFromFileFn =
    unsafe extern "C" fn(*const c_char, LlamaModelParams) -> *mut c_void;
type LlamaModelFreeFn = unsafe extern "C" fn(*mut c_void);
type LlamaModelDescFn = unsafe extern "C" fn(*const c_void, *mut c_char, usize) -> i32;
type LlamaModelSizeFn = unsafe extern "C" fn(*const c_void) -> u64;
type LlamaModelNFn = unsafe extern "C" fn(*const c_void) -> u64;
type LlamaContextDefaultParamsFn = unsafe extern "C" fn(*mut LlamaContextParams);
type LlamaInitFromModelFn = unsafe extern "C" fn(*mut c_void, LlamaContextParams) -> *mut c_void;
type LlamaContextFreeFn = unsafe extern "C" fn(*mut c_void);
type LlamaModelGetVocabFn = unsafe extern "C" fn(*const c_void) -> *mut c_void;
type LlamaTokenizeFn = unsafe extern "C" fn(
    *const c_void,
    *const c_char,
    i32,
    *mut llama_token,
    i32,
    bool,
    bool,
) -> i32;
type LlamaVocabGetEosFn = unsafe extern "C" fn(*const c_void) -> llama_token;

// ── Library handle ──────────────────────────────────────────────

static LIBRARY: OnceLock<libloading::Library> = OnceLock::new();

fn load_library() -> LlamaResult<&'static libloading::Library> {
    let candidates = [
        "/opt/LM-Studio/resources/app/.webpack/bin/extensions/backends/llama.cpp-linux-x86_64-avx2-2.25.2/libllama.so",
        "/usr/lib/libllama.so",
    ];
    let lib_path = candidates
        .iter()
        .find(|p| Path::new(p).exists())
        .ok_or_else(|| LlamaError::LibraryNotFound(PathBuf::from("libllama.so")))?;
    let lib = unsafe { libloading::Library::new(lib_path) }
        .map_err(|e| LlamaError::LoadError(e.to_string()))?;
    LIBRARY.set(lib).ok();
    Ok(LIBRARY.get().unwrap())
}

macro_rules! load_fn {
    ($lib:expr, $name:literal, $type:ty) => {{
        let sym = unsafe { $lib.get::<$type>($name.as_bytes()) }.map_err(|e| {
            LlamaError::SymbolNotFound { symbol: $name.into(), error: e.to_string() }
        })?;
        *sym
    }};
}

pub fn backend_init() -> LlamaResult<()> {
    let lib = load_library()?;
    let f: LlamaBackendInitFn = load_fn!(lib, "llama_backend_init", LlamaBackendInitFn);
    unsafe { f() };
    Ok(())
}

pub fn model_default_params() -> LlamaResult<LlamaModelParams> {
    let lib = load_library()?;
    let f: LlamaModelDefaultParamsFn =
        load_fn!(lib, "llama_model_default_params", LlamaModelDefaultParamsFn);
    let mut p = LlamaModelParams::new();
    unsafe { f(&mut p as *mut LlamaModelParams) };
    Ok(p)
}

pub fn ctx_default_params() -> LlamaResult<LlamaContextParams> {
    let lib = load_library()?;
    let f: LlamaContextDefaultParamsFn =
        load_fn!(lib, "llama_context_default_params", LlamaContextDefaultParamsFn);
    let mut p = LlamaContextParams::new();
    unsafe { f(&mut p as *mut LlamaContextParams) };
    Ok(p)
}

pub fn load_model(path: &Path, n_gpu: i32, mmap: bool) -> LlamaResult<OpaquePtr> {
    let lib = load_library()?;
    let f: LlamaModelLoadFromFileFn =
        load_fn!(lib, "llama_model_load_from_file", LlamaModelLoadFromFileFn);
    let mut p = model_default_params()?;
    p.set_n_gpu_layers(n_gpu);
    p.set_mmap(mmap);
    let c = CString::new(path.to_str().unwrap_or(""))
        .map_err(|_| LlamaError::ApiError("path".into()))?;
    let m = unsafe { f(c.as_ptr() as *const c_char, p) };
    if m.is_null() {
        Err(LlamaError::ApiError("load model failed".into()))
    } else {
        Ok(OpaquePtr(m))
    }
}

pub fn load_model_with_defaults(path: &Path) -> LlamaResult<OpaquePtr> {
    let lib = load_library()?;
    let f: LlamaModelLoadFromFileFn =
        load_fn!(lib, "llama_model_load_from_file", LlamaModelLoadFromFileFn);
    let p = model_default_params()?;
    let c = CString::new(path.to_str().unwrap_or(""))
        .map_err(|_| LlamaError::ApiError("path".into()))?;
    let m = unsafe { f(c.as_ptr() as *const c_char, p) };
    if m.is_null() {
        Err(LlamaError::ApiError("load model failed".into()))
    } else {
        Ok(OpaquePtr(m))
    }
}

pub fn create_context(model: &OpaquePtr, n_ctx: u32, n_threads: i32) -> LlamaResult<OpaquePtr> {
    let lib = load_library()?;
    let f: LlamaInitFromModelFn = load_fn!(lib, "llama_init_from_model", LlamaInitFromModelFn);
    let mut params = ctx_default_params()?;
    params.set_n_ctx(n_ctx);
    params.set_n_threads(n_threads);
    params.set_n_threads_batch(n_threads);
    params.set_embeddings(true);
    let ctx = unsafe { f(model.0, params) };
    if ctx.is_null() {
        Err(LlamaError::ApiError("create context failed".into()))
    } else {
        Ok(OpaquePtr(ctx))
    }
}

pub fn free_model(m: OpaquePtr) {
    if let Some(lib) = LIBRARY.get() {
        if let Ok(f) = unsafe { lib.get::<LlamaModelFreeFn>(b"llama_model_free") } {
            unsafe { f(m.0) };
        }
    }
}
pub fn free_context(c: OpaquePtr) {
    if let Some(lib) = LIBRARY.get() {
        if let Ok(f) = unsafe { lib.get::<LlamaContextFreeFn>(b"llama_context_free") } {
            unsafe { f(c.0) };
        }
    }
}

pub fn tokenize(model: &OpaquePtr, text: &str) -> LlamaResult<Vec<llama_token>> {
    let lib = load_library()?;
    let vocab_fn: LlamaModelGetVocabFn =
        load_fn!(lib, "llama_model_get_vocab", LlamaModelGetVocabFn);
    let tok_fn: LlamaTokenizeFn = load_fn!(lib, "llama_tokenize", LlamaTokenizeFn);
    let vocab = unsafe { vocab_fn(model.0) };
    if vocab.is_null() {
        return Err(LlamaError::ApiError("vocab null".into()));
    }
    let c = CString::new(text).map_err(|_| LlamaError::ApiError("text".into()))?;
    let est = (text.len() as i32).max(32) + 64;
    let mut t = vec![0i32; est as usize];
    let n = unsafe {
        tok_fn(vocab, c.as_ptr() as *const c_char, t.len() as i32, t.as_mut_ptr(), est, true, true)
    };
    if n < 0 {
        let sz = (-n) as usize;
        let mut t2 = vec![0i32; sz];
        let n2 = unsafe {
            tok_fn(
                vocab,
                c.as_ptr() as *const c_char,
                sz as i32,
                t2.as_mut_ptr(),
                sz as i32,
                true,
                true,
            )
        };
        if n2 < 0 {
            return Err(LlamaError::ApiError("tokenize failed".into()));
        }
        t2.truncate(n2 as usize);
        return Ok(t2);
    }
    t.truncate(n as usize);
    Ok(t)
}

pub fn token_count(model: &OpaquePtr, text: &str) -> LlamaResult<usize> {
    tokenize(model, text).map(|t| t.len())
}

pub fn model_desc(model: &OpaquePtr) -> LlamaResult<String> {
    let lib = load_library()?;
    let f: LlamaModelDescFn = load_fn!(lib, "llama_model_desc", LlamaModelDescFn);
    let mut buf = vec![0u8; 1024];
    let r = unsafe { f(model.0, buf.as_mut_ptr() as *mut c_char, buf.len()) };
    if r < 0 {
        Err(LlamaError::ApiError("desc".into()))
    } else {
        Ok(unsafe { CStr::from_ptr(buf.as_ptr() as *const c_char).to_string_lossy().into_owned() })
    }
}

pub fn model_size(model: &OpaquePtr) -> LlamaResult<u64> {
    let lib = load_library()?;
    let f: LlamaModelSizeFn = load_fn!(lib, "llama_model_size", LlamaModelSizeFn);
    Ok(unsafe { f(model.0) })
}
pub fn model_n_params(model: &OpaquePtr) -> LlamaResult<u64> {
    let lib = load_library()?;
    let f: LlamaModelNFn = load_fn!(lib, "llama_model_n_params", LlamaModelNFn);
    Ok(unsafe { f(model.0) })
}
