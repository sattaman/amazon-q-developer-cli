use crate::observability::events::TraceEvent;
use std::path::PathBuf;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

pub struct JsonlSink {
    file_path: PathBuf,
}

impl JsonlSink {
    pub fn new(output_dir: PathBuf, trace_id: Uuid) -> Self {
        let file_path = output_dir.join(format!("{}.jsonl", trace_id));
        Self { file_path }
    }

    pub async fn write(&self, event: &TraceEvent) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)
            .await?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = file.metadata().await?;
            let mut perms = metadata.permissions();
            perms.set_mode(0o600);
            fs::set_permissions(&self.file_path, perms).await?;
        }

        let json = serde_json::to_string(event)?;
        file.write_all(json.as_bytes()).await?;
        file.write_all(b"\n").await?;
        file.flush().await?;

        Ok(())
    }
}
