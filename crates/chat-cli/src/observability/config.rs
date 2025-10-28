use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ObservabilityConfig {
    pub enabled: bool,
    pub output_dir: PathBuf,
    pub langfuse_api_key: Option<String>,
    pub langfuse_public_key: Option<String>,
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
