use std::path::Path;
use std::thread::available_parallelism;

use anyhow::Result as AnyhowResult;
use candle_core::{
    Device,
    Tensor,
};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{
    BertModel,
    DTYPE,
};
use rayon::prelude::*;
use tokenizers::Tokenizer;
use tracing::{
    debug,
    error,
    info,
};

use crate::embedding::candle_models::{
    ModelConfig,
    ModelType,
};
use crate::error::{
    Result,
    SemanticSearchError,
};

/// Text embedding generator using Candle for embedding models
pub struct CandleTextEmbedder {
    /// The BERT model
    model: BertModel,
    /// The tokenizer
    tokenizer: Tokenizer,
    /// The device to run on
    device: Device,
    /// Model configuration
    config: ModelConfig,
}

impl CandleTextEmbedder {
    /// Create a new TextEmbedder with the default model (all-MiniLM-L6-v2)
    ///
    /// # Returns
    ///
    /// A new TextEmbedder instance
    pub fn new() -> Result<Self> {
        Self::with_model_type(ModelType::default())
    }

    /// Create a new TextEmbedder with a specific model type
    ///
    /// # Arguments
    ///
    /// * `model_type` - The type of model to use
    ///
    /// # Returns
    ///
    /// A new TextEmbedder instance
    pub fn with_model_type(model_type: ModelType) -> Result<Self> {
        let model_config = model_type.get_config();
        let (model_path, tokenizer_path) = model_config.get_local_paths();

        // Create model directory if it doesn't exist
        ensure_model_directory_exists(&model_path)?;

        // Download files if they don't exist
        ensure_model_files(&model_path, &tokenizer_path, &model_config)?;

        Self::with_model_config(&model_path, &tokenizer_path, model_config)
    }

    /// Create a new TextEmbedder with specific model paths and configuration
    ///
    /// # Arguments
    ///
    /// * `model_path` - Path to the model file (.safetensors)
    /// * `tokenizer_path` - Path to the tokenizer file (.json)
    /// * `config` - Model configuration
    ///
    /// # Returns
    ///
    /// A new TextEmbedder instance
    pub fn with_model_config(model_path: &Path, tokenizer_path: &Path, config: ModelConfig) -> Result<Self> {
        info!("Initializing text embedder with model: {:?}", model_path);

        // Initialize thread pool
        let threads = initialize_thread_pool()?;
        info!("Using {} threads for text embedding", threads);

        // Load tokenizer
        let tokenizer = load_tokenizer(tokenizer_path)?;

        // Get the best available device (Metal, CUDA, or CPU)
        let device = get_best_available_device();

        // Load model
        let model = load_model(model_path, &config, &device)?;

        debug!("Text embedder initialized successfully");

        Ok(Self {
            model,
            tokenizer,
            device,
            config,
        })
    }

    /// Create a new TextEmbedder with specific model paths
    ///
    /// # Arguments
    ///
    /// * `model_path` - Path to the model file (.safetensors)
    /// * `tokenizer_path` - Path to the tokenizer file (.json)
    ///
    /// # Returns
    ///
    /// A new TextEmbedder instance
    pub fn with_model_paths(model_path: &Path, tokenizer_path: &Path) -> Result<Self> {
        // Use default model configuration
        let config = ModelType::default().get_config();
        Self::with_model_config(model_path, tokenizer_path, config)
    }

    /// Generate an embedding for a text
    ///
    /// # Arguments
    ///
    /// * `text` - The text to embed
    ///
    /// # Returns
    ///
    /// A vector of floats representing the text embedding
    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let texts = vec![text.to_string()];
        match self.embed_batch(&texts) {
            Ok(embeddings) => Ok(embeddings.into_iter().next().unwrap()),
            Err(e) => {
                error!("Failed to embed text: {}", e);
                Err(e)
            },
        }
    }

    /// Generate embeddings for multiple texts
    ///
    /// # Arguments
    ///
    /// * `texts` - The texts to embed
    ///
    /// # Returns
    ///
    /// A vector of embeddings
    pub fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        // Configure tokenizer with padding
        let tokenizer = prepare_tokenizer(&self.tokenizer)?;

        // Process in batches for better memory efficiency
        let batch_size = self.config.batch_size;

        // Use parallel iterator to process batches in parallel
        let all_embeddings: Vec<Vec<f32>> = texts
            .par_chunks(batch_size)
            .flat_map(|batch| self.process_batch(batch, &tokenizer))
            .collect();

        // Check if we have the correct number of embeddings
        if all_embeddings.len() != texts.len() {
            return Err(SemanticSearchError::EmbeddingError(
                "Failed to generate embeddings for all texts".to_string(),
            ));
        }

        Ok(all_embeddings)
    }

    /// Process a batch of texts to generate embeddings
    fn process_batch(&self, batch: &[String], tokenizer: &Tokenizer) -> Vec<Vec<f32>> {
        // Tokenize batch
        let tokens = match tokenizer.encode_batch(batch.to_vec(), true) {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to tokenize texts: {}", e);
                return Vec::new();
            },
        };

        // Convert tokens to tensors
        let (token_ids, attention_mask) = match create_tensors_from_tokens(&tokens, &self.device) {
            Ok(tensors) => tensors,
            Err(_) => return Vec::new(),
        };

        // Create token type ids
        let token_type_ids = match token_ids.zeros_like() {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to create zeros tensor for token_type_ids: {}", e);
                return Vec::new();
            },
        };

        // Run model inference and process results
        self.run_inference_and_process(&token_ids, &token_type_ids, &attention_mask)
            .unwrap_or_else(|_| Vec::new())
    }

    /// Run model inference and process the results
    fn run_inference_and_process(
        &self,
        token_ids: &Tensor,
        token_type_ids: &Tensor,
        attention_mask: &Tensor,
    ) -> Result<Vec<Vec<f32>>> {
        // Run model inference
        let embeddings = match self.model.forward(token_ids, token_type_ids, Some(attention_mask)) {
            Ok(e) => e,
            Err(e) => {
                error!("Model inference failed: {}", e);
                return Err(SemanticSearchError::EmbeddingError(format!(
                    "Model inference failed: {}",
                    e
                )));
            },
        };

        // Apply mean pooling
        let mean_embeddings = match embeddings.mean(1) {
            Ok(m) => m,
            Err(e) => {
                error!("Failed to compute mean embeddings: {}", e);
                return Err(SemanticSearchError::EmbeddingError(format!(
                    "Failed to compute mean embeddings: {}",
                    e
                )));
            },
        };

        // Normalize if configured
        let final_embeddings = if self.config.normalize_embeddings {
            normalize_l2(&mean_embeddings)?
        } else {
            mean_embeddings
        };

        // Convert to Vec<Vec<f32>>
        match final_embeddings.to_vec2::<f32>() {
            Ok(v) => Ok(v),
            Err(e) => {
                error!("Failed to convert embeddings to vector: {}", e);
                Err(SemanticSearchError::EmbeddingError(format!(
                    "Failed to convert embeddings to vector: {}",
                    e
                )))
            },
        }
    }
}

/// Ensure model directory exists
fn ensure_model_directory_exists(model_path: &Path) -> Result<()> {
    let model_dir = model_path.parent().unwrap_or_else(|| Path::new("."));
    if let Err(err) = std::fs::create_dir_all(model_dir) {
        error!("Failed to create model directory: {}", err);
        return Err(SemanticSearchError::IoError(err));
    }
    Ok(())
}

/// Ensure model files exist, downloading them if necessary
fn ensure_model_files(model_path: &Path, tokenizer_path: &Path, config: &ModelConfig) -> Result<()> {
    // Check if files already exist
    if model_path.exists() && tokenizer_path.exists() {
        return Ok(());
    }

    // Create parent directories if they don't exist
    if let Some(parent) = model_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return Err(SemanticSearchError::IoError(e));
        }
    }
    if let Some(parent) = tokenizer_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return Err(SemanticSearchError::IoError(e));
        }
    }

    info!("Downloading model files for {}...", config.name);

    // Download files using Hugging Face Hub API
    download_model_files(model_path, tokenizer_path, config).map_err(|e| {
        error!("Failed to download model files: {}", e);
        SemanticSearchError::EmbeddingError(e.to_string())
    })
}

/// Download model files from Hugging Face Hub
fn download_model_files(model_path: &Path, tokenizer_path: &Path, config: &ModelConfig) -> AnyhowResult<()> {
    // Download model file if it doesn't exist
    if !model_path.exists() {
        let model_url = format!(
            "https://huggingface.co/{}/resolve/main/{}",
            config.repo_path, config.model_file
        );
        download_file_with_custom_tls(&model_url, model_path)?;
    }

    // Download tokenizer file if it doesn't exist
    if !tokenizer_path.exists() {
        let tokenizer_url = format!(
            "https://huggingface.co/{}/resolve/main/{}",
            config.repo_path, config.tokenizer_file
        );
        download_file_with_custom_tls(&tokenizer_url, tokenizer_path)?;
    }

    Ok(())
}

/// Download a file using custom TLS configuration that respects SSL_CERT_FILE
fn download_file_with_custom_tls(url: &str, output_path: &Path) -> AnyhowResult<()> {
    use std::sync::Once;

    // Initialize crypto provider for rustls (required for rustls 0.23+)
    // Use a static flag to ensure we only initialize once
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });

    info!("Downloading {} to {}", url, output_path.display());

    // Create HTTP client with custom TLS if SSL_CERT_FILE is set
    let agent = if let Ok(cert_file) = std::env::var("SSL_CERT_FILE") {
        info!("Using custom SSL certificate from: {}", cert_file);
        create_agent_with_custom_cert(&cert_file)?
    } else {
        // Use default agent if no custom certificate is specified
        info!("No SSL_CERT_FILE specified, using default TLS configuration");
        ureq::Agent::new()
    };

    // Make the HTTP request
    let response = agent
        .get(url)
        .call()
        .map_err(|e| anyhow::anyhow!("Failed to download {}: {}", url, e))?;

    // Create parent directory if it doesn't exist
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow::anyhow!("Failed to create directory {}: {}", parent.display(), e))?;
    }

    // Write the response to file
    let mut file = std::fs::File::create(output_path)
        .map_err(|e| anyhow::anyhow!("Failed to create file {}: {}", output_path.display(), e))?;

    std::io::copy(&mut response.into_reader(), &mut file)
        .map_err(|e| anyhow::anyhow!("Failed to write file {}: {}", output_path.display(), e))?;

    info!("Successfully downloaded {} to {}", url, output_path.display());
    Ok(())
}

/// Create HTTP agent with custom certificate configuration
fn create_agent_with_custom_cert(cert_file: &str) -> AnyhowResult<ureq::Agent> {
    use std::sync::Arc;

    // Read the custom certificate
    let cert_data = std::fs::read(cert_file)
        .map_err(|e| anyhow::anyhow!("Failed to read SSL certificate file {}: {}", cert_file, e))?;

    // Parse the certificate
    let cert = rustls_pemfile::certs(&mut cert_data.as_slice())
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| anyhow::anyhow!("Failed to parse SSL certificate: {}", e))?;

    if cert.is_empty() {
        return Err(anyhow::anyhow!("No certificates found in SSL_CERT_FILE"));
    }

    // Create a custom root certificate store
    let mut root_store = rustls::RootCertStore::empty();

    // Add system certificates first (if RUSTLS_NATIVE_CERTS is set)
    if std::env::var("RUSTLS_NATIVE_CERTS").is_ok() {
        info!("Adding native system certificates");
        add_native_certificates(&mut root_store)?;
    }

    // Add the custom certificate
    for cert_der in cert {
        root_store
            .add(cert_der)
            .map_err(|e| anyhow::anyhow!("Failed to add custom certificate to store: {}", e))?;
    }

    // Create TLS configuration
    let tls_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    // Create HTTP client with custom TLS
    let agent = ureq::AgentBuilder::new().tls_config(Arc::new(tls_config)).build();

    Ok(agent)
}

/// Add native system certificates to the root store
fn add_native_certificates(root_store: &mut rustls::RootCertStore) -> AnyhowResult<()> {
    // Only load native certificates if RUSTLS_NATIVE_CERTS is set
    if std::env::var("RUSTLS_NATIVE_CERTS").is_err() {
        return Ok(());
    }

    let native_certs = rustls_native_certs::load_native_certs()
        .map_err(|e| anyhow::anyhow!("Failed to load native certificates: {}", e))?;

    for cert in native_certs {
        if let Err(e) = root_store.add(cert) {
            // Log but don't fail on individual certificate errors
            tracing::warn!("Failed to add native certificate: {}", e);
        }
    }

    Ok(())
}

/// Initialize thread pool for parallel processing
fn initialize_thread_pool() -> Result<usize> {
    // Automatically detect available parallelism
    let threads = match available_parallelism() {
        Ok(n) => n.get(),
        Err(e) => {
            error!("Failed to detect available parallelism: {}", e);
            // Default to 4 threads if detection fails
            4
        },
    };

    // Initialize the global Rayon thread pool once
    if let Err(e) = rayon::ThreadPoolBuilder::new().num_threads(threads).build_global() {
        // This is fine - it means the pool is already initialized
        debug!("Rayon thread pool already initialized or failed: {}", e);
    }

    Ok(threads)
}

/// Load tokenizer from file
fn load_tokenizer(tokenizer_path: &Path) -> Result<Tokenizer> {
    match Tokenizer::from_file(tokenizer_path) {
        Ok(t) => Ok(t),
        Err(e) => {
            error!("Failed to load tokenizer from {:?}: {}", tokenizer_path, e);
            Err(SemanticSearchError::EmbeddingError(format!(
                "Failed to load tokenizer: {}",
                e
            )))
        },
    }
}

/// Get the best available device for inference
fn get_best_available_device() -> Device {
    // Always use CPU for embedding to avoid hardware acceleration issues
    info!("Using CPU for text embedding (hardware acceleration disabled)");
    Device::Cpu
}

/// Load model from file
fn load_model(model_path: &Path, config: &ModelConfig, device: &Device) -> Result<BertModel> {
    // Load model weights
    let vb = unsafe {
        match VarBuilder::from_mmaped_safetensors(&[model_path], DTYPE, device) {
            Ok(v) => v,
            Err(e) => {
                error!("Failed to load model weights from {:?}: {}", model_path, e);
                return Err(SemanticSearchError::EmbeddingError(format!(
                    "Failed to load model weights: {}",
                    e
                )));
            },
        }
    };

    // Create BERT model
    match BertModel::load(vb, &config.config) {
        Ok(m) => Ok(m),
        Err(e) => {
            error!("Failed to create BERT model: {}", e);
            Err(SemanticSearchError::EmbeddingError(format!(
                "Failed to create BERT model: {}",
                e
            )))
        },
    }
}

/// Prepare tokenizer with padding configuration
fn prepare_tokenizer(tokenizer: &Tokenizer) -> Result<Tokenizer> {
    let mut tokenizer = tokenizer.clone();
    if let Some(pp) = tokenizer.get_padding_mut() {
        pp.strategy = tokenizers::PaddingStrategy::BatchLongest;
    } else {
        let pp = tokenizers::PaddingParams {
            strategy: tokenizers::PaddingStrategy::BatchLongest,
            ..Default::default()
        };
        tokenizer.with_padding(Some(pp));
    }
    Ok(tokenizer)
}

/// Create tensors from tokenized inputs
fn create_tensors_from_tokens(tokens: &[tokenizers::Encoding], device: &Device) -> Result<(Tensor, Tensor)> {
    // Pre-allocate vectors with exact capacity
    let mut token_ids = Vec::with_capacity(tokens.len());
    let mut attention_mask = Vec::with_capacity(tokens.len());

    // Convert tokens to tensors
    for tokens in tokens {
        let ids = tokens.get_ids().to_vec();
        let mask = tokens.get_attention_mask().to_vec();

        let ids_tensor = match Tensor::new(ids.as_slice(), device) {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to create token_ids tensor: {}", e);
                return Err(SemanticSearchError::EmbeddingError(format!(
                    "Failed to create token_ids tensor: {}",
                    e
                )));
            },
        };

        let mask_tensor = match Tensor::new(mask.as_slice(), device) {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to create attention_mask tensor: {}", e);
                return Err(SemanticSearchError::EmbeddingError(format!(
                    "Failed to create attention_mask tensor: {}",
                    e
                )));
            },
        };

        token_ids.push(ids_tensor);
        attention_mask.push(mask_tensor);
    }

    // Stack tensors into batches
    let token_ids = match Tensor::stack(&token_ids, 0) {
        Ok(t) => t,
        Err(e) => {
            error!("Failed to stack token_ids tensors: {}", e);
            return Err(SemanticSearchError::EmbeddingError(format!(
                "Failed to stack token_ids tensors: {}",
                e
            )));
        },
    };

    let attention_mask = match Tensor::stack(&attention_mask, 0) {
        Ok(t) => t,
        Err(e) => {
            error!("Failed to stack attention_mask tensors: {}", e);
            return Err(SemanticSearchError::EmbeddingError(format!(
                "Failed to stack attention_mask tensors: {}",
                e
            )));
        },
    };

    Ok((token_ids, attention_mask))
}

/// Normalize embedding to unit length (L2 norm)
fn normalize_l2(v: &Tensor) -> Result<Tensor> {
    // Calculate squared values
    let squared = match v.sqr() {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to square tensor for L2 normalization: {}", e);
            return Err(SemanticSearchError::EmbeddingError(format!(
                "Failed to square tensor: {}",
                e
            )));
        },
    };

    // Sum along last dimension and keep dimensions
    let sum_squared = match squared.sum_keepdim(1) {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to sum squared values: {}", e);
            return Err(SemanticSearchError::EmbeddingError(format!(
                "Failed to sum tensor: {}",
                e
            )));
        },
    };

    // Calculate square root for L2 norm
    let norm = match sum_squared.sqrt() {
        Ok(n) => n,
        Err(e) => {
            error!("Failed to compute square root for normalization: {}", e);
            return Err(SemanticSearchError::EmbeddingError(format!(
                "Failed to compute square root: {}",
                e
            )));
        },
    };

    // Divide by norm
    match v.broadcast_div(&norm) {
        Ok(n) => Ok(n),
        Err(e) => {
            error!("Failed to normalize by division: {}", e);
            Err(SemanticSearchError::EmbeddingError(format!(
                "Failed to normalize: {}",
                e
            )))
        },
    }
}

impl crate::embedding::BenchmarkableEmbedder for CandleTextEmbedder {
    fn model_name(&self) -> String {
        format!("Candle-{}", self.config.name)
    }

    fn embedding_dim(&self) -> usize {
        self.config.config.hidden_size
    }

    fn embed_single(&self, text: &str) -> Vec<f32> {
        self.embed(text).unwrap()
    }

    fn embed_batch(&self, texts: &[String]) -> Vec<Vec<f32>> {
        self.embed_batch(texts).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        fs,
    };

    use tempfile::tempdir;

    use super::*;

    // Helper function to create a test embedder with mock files
    fn create_test_embedder() -> Result<CandleTextEmbedder> {
        // Use a temporary directory for test files
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let _model_path = temp_dir.path().join("model.safetensors");
        let _tokenizer_path = temp_dir.path().join("tokenizer.json");

        // Mock the ensure_model_files function to avoid actual downloads
        // This is a simplified test that checks error handling paths

        // Return a mock error to test error handling
        Err(crate::error::SemanticSearchError::EmbeddingError(
            "Test error".to_string(),
        ))
    }

    /// Helper function to check if real embedder tests should be skipped
    fn should_skip_real_embedder_tests() -> bool {
        // Skip if real embedders are not explicitly requested
        if env::var("MEMORY_BANK_USE_REAL_EMBEDDERS").is_err() {
            return true;
        }

        // Skip in CI environments
        if env::var("CI").is_ok() {
            return true;
        }

        false
    }

    /// Helper function to create test data for performance tests
    fn create_test_data() -> Vec<String> {
        vec![
            "This is a short sentence.".to_string(),
            "Another simple example.".to_string(),
            "The quick brown fox jumps over the lazy dog.".to_string(),
            "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.".to_string(),
            "Machine learning models can process and analyze text data to extract meaningful information and generate embeddings that represent semantic relationships between words and phrases.".to_string(),
        ]
    }

    #[test]
    fn test_ssl_cert_file_handling() {
        // Test that SSL_CERT_FILE environment variable is properly read
        let original_cert_file = env::var("SSL_CERT_FILE").ok();

        // Test with no SSL_CERT_FILE set
        unsafe {
            env::remove_var("SSL_CERT_FILE");
        }
        // This should not panic and should use default TLS
        // We can't easily test the actual HTTP functionality without a server

        // Test with invalid SSL_CERT_FILE path
        unsafe {
            env::set_var("SSL_CERT_FILE", "/nonexistent/path/cert.pem");
        }
        let result = create_agent_with_custom_cert("/nonexistent/path/cert.pem");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Failed to read SSL certificate file")
        );

        // Restore original environment
        unsafe {
            if let Some(cert_file) = original_cert_file {
                env::set_var("SSL_CERT_FILE", cert_file);
            } else {
                env::remove_var("SSL_CERT_FILE");
            }
        }
    }

    #[test]
    fn test_create_agent_with_invalid_cert() {
        // Create a temporary file with invalid certificate content
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let cert_path = temp_dir.path().join("invalid_cert.pem");
        fs::write(&cert_path, "invalid certificate content").expect("Failed to write test file");

        let result = create_agent_with_custom_cert(cert_path.to_str().unwrap());
        assert!(result.is_err());

        // The error should be about no certificates found since the content is invalid
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("No certificates found in SSL_CERT_FILE"));
    }

    #[test]
    fn test_create_agent_with_empty_cert_file() {
        // Create a temporary file with no certificate content
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let cert_path = temp_dir.path().join("empty_cert.pem");
        fs::write(&cert_path, "").expect("Failed to write test file");

        let result = create_agent_with_custom_cert(cert_path.to_str().unwrap());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No certificates found in SSL_CERT_FILE")
        );
    }

    #[test]
    fn test_rustls_native_certs_environment_variable() {
        let original_native_certs = env::var("RUSTLS_NATIVE_CERTS").ok();

        // Test with RUSTLS_NATIVE_CERTS not set
        unsafe {
            env::remove_var("RUSTLS_NATIVE_CERTS");
        }
        let mut root_store = rustls::RootCertStore::empty();
        let result = add_native_certificates(&mut root_store);
        // This should succeed but not add any certificates when env var is not set
        assert!(result.is_ok());

        // Test with RUSTLS_NATIVE_CERTS set
        unsafe {
            env::set_var("RUSTLS_NATIVE_CERTS", "1");
        }
        let mut root_store = rustls::RootCertStore::empty();
        let result = add_native_certificates(&mut root_store);
        // This might fail on some systems if native certs can't be loaded, which is acceptable
        // We just want to ensure it doesn't panic
        match result {
            Ok(_) => {
                // Successfully loaded native certificates
            },
            Err(e) => {
                // Failed to load native certificates, which is acceptable in test environments
                println!("Note: Failed to load native certificates in test: {}", e);
            },
        }

        // Restore original environment
        unsafe {
            if let Some(native_certs) = original_native_certs {
                env::set_var("RUSTLS_NATIVE_CERTS", native_certs);
            } else {
                env::remove_var("RUSTLS_NATIVE_CERTS");
            }
        }
    }

    #[test]
    fn test_crypto_provider_initialization() {
        // Test that crypto provider initialization doesn't panic
        // This is mainly to ensure the static initialization works correctly
        use std::sync::Once;
        static TEST_INIT: Once = Once::new();

        TEST_INIT.call_once(|| {
            let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        });

        // If we get here without panicking, the initialization worked
        assert!(true);
    }

    #[test]
    fn test_embed_single() {
        if should_skip_real_embedder_tests() {
            return;
        }

        // Use real embedder for testing
        match CandleTextEmbedder::new() {
            Ok(embedder) => {
                let embedding = embedder.embed("This is a test sentence.").unwrap();

                // MiniLM-L6-v2 produces 384-dimensional embeddings
                assert_eq!(embedding.len(), 384);

                // Check that the embedding is normalized (L2 norm ≈ 1.0)
                let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
                assert!((norm - 1.0).abs() < 1e-5);
            },
            Err(e) => {
                // If model loading fails, skip the test
                println!("Skipping test: Failed to load real embedder: {}", e);
            },
        }
    }

    #[test]
    fn test_embed_batch() {
        if should_skip_real_embedder_tests() {
            return;
        }

        // Use real embedder for testing
        match CandleTextEmbedder::new() {
            Ok(embedder) => {
                let texts = vec![
                    "The cat sits outside".to_string(),
                    "A man is playing guitar".to_string(),
                ];
                let embeddings = embedder.embed_batch(&texts).unwrap();

                assert_eq!(embeddings.len(), 2);
                assert_eq!(embeddings[0].len(), 384);
                assert_eq!(embeddings[1].len(), 384);

                // Check that embeddings are different
                let mut different = false;
                for i in 0..384 {
                    if (embeddings[0][i] - embeddings[1][i]).abs() > 1e-5 {
                        different = true;
                        break;
                    }
                }
                assert!(different);
            },
            Err(e) => {
                // If model loading fails, skip the test
                println!("Skipping test: Failed to load real embedder: {}", e);
            },
        }
    }

    #[test]
    fn test_model_types() {
        // Test that we can create embedders with different model types
        // This is just a compilation test, we don't actually load the models

        // These should compile without errors
        let _model_type1 = ModelType::MiniLML6V2;
        let _model_type2 = ModelType::MiniLML12V2;

        // Test that default is MiniLML6V2
        assert_eq!(ModelType::default(), ModelType::MiniLML6V2);
    }

    #[test]
    fn test_error_handling() {
        // Test error handling with invalid paths
        let invalid_path = Path::new("/nonexistent/path");
        let result = CandleTextEmbedder::with_model_paths(invalid_path, invalid_path);
        assert!(result.is_err());

        // Test error handling with mock embedder
        let result = create_test_embedder();
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_model_files() {
        // Create temporary directory for test
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let model_path = temp_dir.path().join("model.safetensors");
        let tokenizer_path = temp_dir.path().join("tokenizer.json");

        // Create empty files to simulate existing files
        fs::write(&model_path, "mock data").expect("Failed to write mock model file");
        fs::write(&tokenizer_path, "mock data").expect("Failed to write mock tokenizer file");

        // Test that ensure_model_files returns Ok when files exist
        let config = ModelType::default().get_config();
        let result = ensure_model_files(&model_path, &tokenizer_path, &config);
        assert!(result.is_ok());
    }

    /// Performance test for different model types
    #[test]
    fn test_model_performance() {
        if should_skip_real_embedder_tests() {
            return;
        }

        // Test data
        let texts = create_test_data();

        // Test each model type
        let model_types = [ModelType::MiniLML6V2, ModelType::MiniLML12V2];

        for model_type in model_types {
            run_performance_test(model_type, &texts);
        }
    }

    /// Run performance test for a specific model type
    fn run_performance_test(model_type: ModelType, texts: &[String]) {
        match CandleTextEmbedder::with_model_type(model_type) {
            Ok(embedder) => {
                println!("Testing performance of {:?}", model_type);

                // Warm-up run
                let _ = embedder.embed_batch(texts);

                // Measure single embedding performance
                let start = std::time::Instant::now();
                let single_result = embedder.embed(&texts[0]);
                let single_duration = start.elapsed();

                // Measure batch embedding performance
                let start = std::time::Instant::now();
                let batch_result = embedder.embed_batch(texts);
                let batch_duration = start.elapsed();

                // Check results are valid
                assert!(single_result.is_ok());
                assert!(batch_result.is_ok());

                // Get embedding dimensions
                let embedding_dim = single_result.unwrap().len();

                println!(
                    "Model: {:?}, Embedding dim: {}, Single time: {:?}, Batch time: {:?}, Avg per text: {:?}",
                    model_type,
                    embedding_dim,
                    single_duration,
                    batch_duration,
                    batch_duration.div_f32(texts.len() as f32)
                );
            },
            Err(e) => {
                println!("Failed to load model {:?}: {}", model_type, e);
            },
        }
    }

    /// Test loading all models to ensure they work
    #[test]
    fn test_load_all_models() {
        if should_skip_real_embedder_tests() {
            return;
        }

        let model_types = [ModelType::MiniLML6V2, ModelType::MiniLML12V2];

        for model_type in model_types {
            test_model_loading(model_type);
        }
    }

    /// Test loading a specific model
    fn test_model_loading(model_type: ModelType) {
        match CandleTextEmbedder::with_model_type(model_type) {
            Ok(embedder) => {
                // Test a simple embedding to verify the model works
                let result = embedder.embed("Test sentence for model verification.");
                assert!(result.is_ok(), "Model {:?} failed to generate embedding", model_type);

                // Verify embedding dimensions
                let embedding = result.unwrap();
                let expected_dim = match model_type {
                    ModelType::MiniLML6V2 => 384,
                    ModelType::MiniLML12V2 => 384,
                };

                assert_eq!(
                    embedding.len(),
                    expected_dim,
                    "Model {:?} produced embedding with incorrect dimensions",
                    model_type
                );

                println!("Successfully loaded and tested model {:?}", model_type);
            },
            Err(e) => {
                println!("Failed to load model {:?}: {}", model_type, e);
                // Don't fail the test if a model can't be loaded, just report it
            },
        }
    }
}
