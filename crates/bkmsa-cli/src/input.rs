use crate::error::CliError;
use std::path::Path;
use tokio::io::AsyncReadExt;
use url::Url;

const SPARK_CONTENT_ORIGIN: &str = "https://spark-usercontent.lucko.me/";
const MAX_REPORT_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ReportInput {
    pub bytes: Vec<u8>,
    pub source: String,
    pub hint: String,
}

pub async fn load_report(source: &str) -> Result<ReportInput, CliError> {
    let source = source.trim();
    if source.is_empty() {
        return Err(CliError::Input("source cannot be empty".into()));
    }

    if source == "-" {
        let mut bytes = Vec::new();
        tokio::io::stdin()
            .take((MAX_REPORT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .await
            .map_err(|error| CliError::Input(format!("stdin: {error}")))?;
        ensure_size(bytes.len(), "stdin")?;
        return Ok(ReportInput {
            bytes,
            source: "stdin".into(),
            hint: String::new(),
        });
    }

    let path = Path::new(source);
    let metadata = match tokio::fs::metadata(path).await {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(CliError::Input(format!("{}: {error}", path.display())));
        }
    };
    if metadata.as_ref().is_some_and(|value| value.is_file()) {
        if metadata
            .as_ref()
            .is_some_and(|value| value.len() > MAX_REPORT_BYTES as u64)
        {
            return Err(CliError::Input(format!(
                "{} exceeds the {} MiB report limit",
                path.display(),
                MAX_REPORT_BYTES / 1024 / 1024
            )));
        }
        let mut bytes = Vec::new();
        tokio::fs::File::open(path)
            .await
            .map_err(|error| CliError::Input(format!("{}: {error}", path.display())))?
            .take((MAX_REPORT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .await
            .map_err(|error| CliError::Input(format!("{}: {error}", path.display())))?;
        ensure_size(bytes.len(), &path.display().to_string())?;
        return Ok(ReportInput {
            bytes,
            source: path.display().to_string(),
            hint: path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_owned(),
        });
    }

    if !source.contains("://") && looks_like_path(source) {
        return Err(CliError::Input(format!("file does not exist: {source}")));
    }

    let url = resolve_spark_report_url(source)?;
    let display_url = redact_url(&url);
    let response = reqwest::Client::builder()
        .user_agent(concat!("bkmsa/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 || !allowed_report_url(attempt.url()) {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|error| CliError::Input(error.to_string()))?
        .get(url.clone())
        .send()
        .await
        .map_err(|error| CliError::Input(format!("{display_url}: {error}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(CliError::Input(format!(
            "{display_url} returned HTTP {status}"
        )));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_REPORT_BYTES as u64)
    {
        return Err(CliError::Input(format!(
            "{display_url} exceeds the {} MiB report limit",
            MAX_REPORT_BYTES / 1024 / 1024
        )));
    }
    let final_url = response.url().clone();
    let mut response = response;
    let mut bytes = Vec::with_capacity(
        response
            .content_length()
            .unwrap_or_default()
            .min(MAX_REPORT_BYTES as u64) as usize,
    );
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| CliError::Input(format!("{display_url}: {error}")))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_REPORT_BYTES {
            return Err(CliError::Input(format!(
                "{display_url} exceeds the {} MiB report limit",
                MAX_REPORT_BYTES / 1024 / 1024
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    let hint = Path::new(final_url.path())
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_owned)
        .unwrap_or_default();
    Ok(ReportInput {
        bytes,
        source: redact_url(final_url.as_str()),
        hint,
    })
}

pub fn resolve_spark_report_url(input: &str) -> Result<String, CliError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(CliError::Input("spark URL or key cannot be empty".into()));
    }

    match Url::parse(trimmed) {
        Ok(url) => {
            if !matches!(url.scheme(), "http" | "https") {
                return Err(CliError::Input(format!(
                    "unsupported URL scheme: {}",
                    url.scheme()
                )));
            }
            let host = url.host_str().unwrap_or_default();
            if host == "spark-usercontent.lucko.me" {
                let mut url = url;
                url.set_scheme("https")
                    .map_err(|_| CliError::Input("unable to normalize report URL".into()))?;
                if !url.username().is_empty() || url.password().is_some() {
                    return Err(CliError::Input(
                        "report URL must not contain credentials".into(),
                    ));
                }
                if url.port_or_known_default() != Some(443) {
                    return Err(CliError::Input(
                        "spark report URL must use the default HTTPS port".into(),
                    ));
                }
                return Ok(url.to_string());
            }
            if host == "spark.lucko.me" {
                let key = url
                    .path_segments()
                    .into_iter()
                    .flatten()
                    .rfind(|part| !part.is_empty() && *part != "viewer" && *part != "profile")
                    .map(|part| {
                        percent_encoding::percent_decode_str(part)
                            .decode_utf8_lossy()
                            .into_owned()
                    })
                    .or_else(|| {
                        url.query_pairs()
                            .find(|(name, _)| name == "id" || name == "key")
                            .map(|(_, value)| value.into_owned())
                    })
                    .ok_or_else(|| {
                        CliError::Input("unable to extract report key from spark viewer URL".into())
                    })?;
                return content_url(&key);
            }
            Err(CliError::Input(format!("unsupported report host: {host}")))
        }
        Err(_) if trimmed.contains("://") => Err(CliError::Input("invalid report URL".into())),
        Err(_) => content_url(trimmed.trim_matches('/')),
    }
}

fn allowed_report_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("spark-usercontent.lucko.me")
        && url.port_or_known_default() == Some(443)
}

fn content_url(key: &str) -> Result<String, CliError> {
    if key.is_empty() || key.len() > 256 || key.chars().any(char::is_control) {
        return Err(CliError::Input("invalid spark report key".into()));
    }
    let mut url = Url::parse(SPARK_CONTENT_ORIGIN).expect("static spark content URL");
    url.path_segments_mut()
        .expect("spark content URL is hierarchical")
        .push(key);
    Ok(url.into())
}

fn redact_url(value: &str) -> String {
    let Ok(mut url) = Url::parse(value) else {
        return value.to_owned();
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.into()
}

fn ensure_size(length: usize, source: &str) -> Result<(), CliError> {
    if length > MAX_REPORT_BYTES {
        Err(CliError::Input(format!(
            "{source} exceeds the {} MiB report limit",
            MAX_REPORT_BYTES / 1024 / 1024
        )))
    } else {
        Ok(())
    }
}

fn looks_like_path(source: &str) -> bool {
    let lower = source.to_ascii_lowercase();
    source.starts_with('.')
        || source.starts_with('/')
        || source.contains(std::path::MAIN_SEPARATOR)
        || [
            "sparkprofile",
            "sparkheap",
            "pb",
            "protobuf",
            "txt",
            "log",
            "md",
        ]
        .iter()
        .any(|extension| lower.ends_with(&format!(".{extension}")))
}
