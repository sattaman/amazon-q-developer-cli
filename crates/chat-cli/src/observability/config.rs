use std::path::PathBuf;

/// Configuration for observability and trace collection.
#[derive(Debug, Clone)]
pub struct ObservabilityConfig {
    /// Whether observability tracing is enabled
    pub enabled: bool,
    /// Directory where JSONL trace files will be written
    pub output_dir: PathBuf,
    /// Langfuse API secret key (optional, for Langfuse integration)
    pub langfuse_api_key: Option<String>,
    /// Langfuse API public key (optional, for Langfuse integration)
    pub langfuse_public_key: Option<String>,
    /// Langfuse API URL (optional, defaults to cloud.langfuse.com)
    pub langfuse_api_url: Option<String>,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            output_dir: dirs::home_dir()
                .unwrap_or_default()
                .join(".q")
                .join("traces"),
            langfuse_api_key: None,
            langfuse_public_key: None,
            langfuse_api_url: None,
        }
    }
}
