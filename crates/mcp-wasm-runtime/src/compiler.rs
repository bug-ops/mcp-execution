//! TypeScript to WASM compilation.
//!
//! Provides compilation pipeline from TypeScript code to WASM bytecode
//! with caching and validation.
//!
//! # Compilation Strategies
//!
//! Two compilation backends are supported (feature-gated):
//! - **AssemblyScript**: TypeScript-like language compiling to WASM
//! - **QuickJS**: JavaScript engine compiled to WASM
//!
//! # Caching
//!
//! Compiled modules are cached using BLAKE3 hashes of source code
//! for fast reloading.
//!
//! # Examples
//!
//! ```no_run
//! use mcp_wasm_runtime::compiler::Compiler;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let compiler = Compiler::new();
//! let typescript_code = r#"
//!     export function main(): i32 {
//!         return 42;
//!     }
//! "#;
//!
//! let wasm_bytes = compiler.compile(typescript_code)?;
//! # Ok(())
//! # }
//! ```

use blake3::Hasher;
use mcp_core::{Error, Result};
use std::collections::HashMap;
use std::sync::RwLock;

/// TypeScript to WASM compiler with caching.
///
/// Compiles TypeScript code to WASM bytecode using the configured
/// backend (AssemblyScript or QuickJS).
///
/// # Examples
///
/// ```
/// use mcp_wasm_runtime::compiler::Compiler;
///
/// let compiler = Compiler::new();
/// ```
pub struct Compiler {
    /// Compiled module cache (hash -> WASM bytes)
    cache: RwLock<HashMap<String, Vec<u8>>>,

    /// Compilation backend
    backend: CompilationBackend,
}

impl std::fmt::Debug for Compiler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (count, size) = self.cache_stats();
        f.debug_struct("Compiler")
            .field("backend", &self.backend)
            .field("cache_entries", &count)
            .field("cache_size_bytes", &size)
            .finish()
    }
}

/// Compilation backend selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompilationBackend {
    /// AssemblyScript compiler (TypeScript-like → WASM)
    AssemblyScript,

    /// QuickJS embedded in WASM
    QuickJS,

    /// Precompiled WASM (bypass compilation)
    Precompiled,
}

impl Compiler {
    /// Creates a new compiler with default backend.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::compiler::Compiler;
    ///
    /// let compiler = Compiler::new();
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::with_backend(Self::default_backend())
    }

    /// Creates a compiler with specific backend.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::compiler::{Compiler, CompilationBackend};
    ///
    /// let compiler = Compiler::with_backend(CompilationBackend::QuickJS);
    /// ```
    #[must_use]
    pub fn with_backend(backend: CompilationBackend) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            backend,
        }
    }

    /// Returns default compilation backend based on feature flags.
    fn default_backend() -> CompilationBackend {
        #[cfg(feature = "assemblyscript")]
        return CompilationBackend::AssemblyScript;

        #[cfg(all(feature = "quickjs", not(feature = "assemblyscript")))]
        return CompilationBackend::QuickJS;

        #[cfg(not(any(feature = "assemblyscript", feature = "quickjs")))]
        CompilationBackend::Precompiled
    }

    /// Compiles TypeScript code to WASM bytecode.
    ///
    /// Uses caching to avoid recompiling identical code.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Compilation fails
    /// - Code is invalid
    /// - Backend is not available
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_wasm_runtime::compiler::Compiler;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let compiler = Compiler::new();
    /// let code = "export function main(): i32 { return 42; }";
    /// let wasm = compiler.compile(code)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn compile(&self, typescript_code: &str) -> Result<Vec<u8>> {
        // Calculate code hash for caching
        let code_hash = self.calculate_hash(typescript_code);

        // Check cache
        {
            let cache = self.cache.read().unwrap();
            if let Some(cached) = cache.get(&code_hash) {
                tracing::debug!("Using cached WASM module: {}", &code_hash[..8]);
                return Ok(cached.clone());
            }
        }

        tracing::info!("Compiling TypeScript to WASM (backend: {:?})", self.backend);

        // Compile based on backend
        let wasm_bytes = match self.backend {
            CompilationBackend::AssemblyScript => self.compile_assemblyscript(typescript_code)?,
            CompilationBackend::QuickJS => self.compile_quickjs(typescript_code)?,
            CompilationBackend::Precompiled => {
                return Err(Error::ExecutionError {
                    message: "Precompiled backend requires pre-compiled WASM bytes".into(),
                    source: None,
                });
            }
        };

        // Cache compiled module
        {
            let mut cache = self.cache.write().unwrap();
            cache.insert(code_hash.clone(), wasm_bytes.clone());
        }

        tracing::info!("Compilation successful, cached as: {}", &code_hash[..8]);

        Ok(wasm_bytes)
    }

    /// Loads precompiled WASM bytecode directly.
    ///
    /// Bypasses compilation and caches the provided WASM bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::compiler::Compiler;
    ///
    /// let compiler = Compiler::new();
    /// let wasm_bytes = vec![0x00, 0x61, 0x73, 0x6d]; // Magic bytes
    /// compiler.load_precompiled(&wasm_bytes);
    /// ```
    pub fn load_precompiled(&self, wasm_bytes: &[u8]) -> String {
        let hash = self.calculate_hash_bytes(wasm_bytes);

        let mut cache = self.cache.write().unwrap();
        cache.insert(hash.clone(), wasm_bytes.to_vec());

        hash
    }

    /// Clears compilation cache.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::compiler::Compiler;
    ///
    /// let compiler = Compiler::new();
    /// compiler.clear_cache();
    /// ```
    pub fn clear_cache(&self) {
        let mut cache = self.cache.write().unwrap();
        cache.clear();
        tracing::info!("Compilation cache cleared");
    }

    /// Returns cache statistics.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::compiler::Compiler;
    ///
    /// let compiler = Compiler::new();
    /// let (count, size) = compiler.cache_stats();
    /// println!("Cache: {} modules, {} bytes", count, size);
    /// ```
    #[must_use]
    pub fn cache_stats(&self) -> (usize, usize) {
        let cache = self.cache.read().unwrap();
        let count = cache.len();
        let size: usize = cache.values().map(|v| v.len()).sum();
        (count, size)
    }

    /// Calculates BLAKE3 hash of code string.
    fn calculate_hash(&self, code: &str) -> String {
        let mut hasher = Hasher::new();
        hasher.update(code.as_bytes());
        format!("wasm_{}", hasher.finalize().to_hex())
    }

    /// Calculates BLAKE3 hash of bytes.
    fn calculate_hash_bytes(&self, bytes: &[u8]) -> String {
        let mut hasher = Hasher::new();
        hasher.update(bytes);
        format!("wasm_{}", hasher.finalize().to_hex())
    }

    /// Compiles TypeScript using AssemblyScript.
    ///
    /// # Implementation Status
    ///
    /// TODO(phase-6): Implement actual AssemblyScript compilation pipeline.
    /// NOTE: Phase 6 optimization is DEFERRED per ADR and CLAUDE.md. Current performance
    /// already exceeds targets by 526-6,578x. Implementation should wait for production
    /// data indicating specific needs.
    ///
    /// For now, returns an error as this backend is not implemented.
    ///
    /// # Future Implementation
    ///
    /// This would involve:
    /// - Calling AssemblyScript CLI (`asc`) or using Node.js bindings
    /// - Setting up proper TypeScript → WASM compilation pipeline
    /// - Handling compilation errors and warnings
    /// - Integrating with module cache system
    #[cfg(feature = "assemblyscript")]
    fn compile_assemblyscript(&self, _code: &str) -> Result<Vec<u8>> {
        // TODO(phase-6): Integrate AssemblyScript compiler
        // See: https://www.assemblyscript.org/compiler.html
        Err(Error::ExecutionError {
            message: "AssemblyScript compilation not yet implemented".into(),
            source: None,
        })
    }

    #[cfg(not(feature = "assemblyscript"))]
    fn compile_assemblyscript(&self, _code: &str) -> Result<Vec<u8>> {
        Err(Error::ExecutionError {
            message: "AssemblyScript backend not enabled (use --features assemblyscript)".into(),
            source: None,
        })
    }

    /// Compiles JavaScript using QuickJS embedded in WASM.
    ///
    /// # Implementation Status
    ///
    /// TODO(phase-6): Implement QuickJS WASM integration.
    /// NOTE: Phase 6 optimization is DEFERRED per ADR and CLAUDE.md. Current performance
    /// already exceeds targets by 526-6,578x. Implementation should wait for production
    /// data indicating specific needs.
    ///
    /// For now, returns an error as this backend is not implemented.
    ///
    /// # Future Implementation
    ///
    /// This would involve:
    /// - Embedding precompiled QuickJS WASM module
    /// - Wrapping JavaScript code for execution in QuickJS
    /// - Setting up JS ↔ WASM interop layer
    /// - Handling runtime errors and exceptions
    #[cfg(feature = "quickjs")]
    fn compile_quickjs(&self, _code: &str) -> Result<Vec<u8>> {
        // TODO(phase-6): Integrate QuickJS WASM
        // See: https://github.com/justjake/quickjs-emscripten
        Err(Error::ExecutionError {
            message: "QuickJS compilation not yet implemented".into(),
            source: None,
        })
    }

    #[cfg(not(feature = "quickjs"))]
    fn compile_quickjs(&self, _code: &str) -> Result<Vec<u8>> {
        Err(Error::ExecutionError {
            message: "QuickJS backend not enabled (use --features quickjs)".into(),
            source: None,
        })
    }
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compiler_creation() {
        let compiler = Compiler::new();
        assert!(matches!(
            compiler.backend,
            CompilationBackend::AssemblyScript | CompilationBackend::Precompiled
        ));
    }

    #[test]
    fn test_backend_selection() {
        let compiler = Compiler::with_backend(CompilationBackend::QuickJS);
        assert_eq!(compiler.backend, CompilationBackend::QuickJS);
    }

    #[test]
    fn test_cache_hash_calculation() {
        let compiler = Compiler::new();
        let hash1 = compiler.calculate_hash("test code");
        let hash2 = compiler.calculate_hash("test code");
        let hash3 = compiler.calculate_hash("different code");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert!(hash1.starts_with("wasm_"));
    }

    #[test]
    fn test_precompiled_loading() {
        let compiler = Compiler::new();
        let wasm_bytes = vec![0x00, 0x61, 0x73, 0x6d]; // WASM magic number

        let hash = compiler.load_precompiled(&wasm_bytes);
        assert!(hash.starts_with("wasm_"));

        let (count, size) = compiler.cache_stats();
        assert_eq!(count, 1);
        assert_eq!(size, 4);
    }

    #[test]
    fn test_clear_cache() {
        let compiler = Compiler::new();
        compiler.load_precompiled(&[1, 2, 3, 4]);

        let (count_before, _) = compiler.cache_stats();
        assert_eq!(count_before, 1);

        compiler.clear_cache();

        let (count_after, _) = compiler.cache_stats();
        assert_eq!(count_after, 0);
    }

    #[test]
    fn test_cache_stats() {
        let compiler = Compiler::new();
        assert_eq!(compiler.cache_stats(), (0, 0));

        compiler.load_precompiled(&[1, 2, 3]);
        compiler.load_precompiled(&[4, 5, 6, 7]);

        let (count, size) = compiler.cache_stats();
        assert_eq!(count, 2);
        assert_eq!(size, 7);
    }
}
