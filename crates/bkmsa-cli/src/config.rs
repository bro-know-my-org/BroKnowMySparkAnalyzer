use crate::error::CliError;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;

const MAX_CONFIG_BYTES: usize = 1024 * 1024;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub temperature: Option<f32>,
}

pub fn default_path() -> Option<PathBuf> {
    dirs::config_dir().map(|directory| directory.join("bkmsa").join("config.toml"))
}

pub async fn load(explicit: Option<&Path>) -> Result<FileConfig, CliError> {
    let path = explicit.map(Path::to_path_buf).or_else(default_path);
    let Some(path) = path else {
        return Ok(FileConfig::default());
    };

    let text = match read_with_limit(&path).await {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && explicit.is_none() => {
            return Ok(FileConfig::default());
        }
        Err(error) => {
            return Err(CliError::Config(format!(
                "unable to read config {}: {error}",
                path.display()
            )))
        }
    };
    toml::from_str(&text)
        .map_err(|error| CliError::Config(format!("invalid config {}: {error}", path.display())))
}

async fn read_with_limit(path: &Path) -> std::io::Result<String> {
    let path = path.to_path_buf();
    let file = tokio::task::spawn_blocking(move || open_regular_file(&path))
        .await
        .map_err(std::io::Error::other)??;
    let mut bytes = Vec::new();
    tokio::fs::File::from_std(file)
        .take((MAX_CONFIG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "config exceeds 1 MiB limit",
        ));
    }
    String::from_utf8(bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

fn open_regular_file(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "config path must not be a reparse point",
            ));
        }
    }
    if !metadata.file_type().is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "config path must be a regular file",
        ));
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn loads_toml_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        tokio::fs::write(
            &path,
            "base_url = \"https://example.test/v1\"\nmodel = \"test-model\"\ntemperature = 0.1\n",
        )
        .await
        .unwrap();
        let config = load(Some(&path)).await.unwrap();
        assert_eq!(config.model.as_deref(), Some("test-model"));
        assert_eq!(config.temperature, Some(0.1));
    }
}
