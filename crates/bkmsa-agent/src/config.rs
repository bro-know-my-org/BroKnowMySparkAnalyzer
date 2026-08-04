#[cfg(feature = "native-client")]
use std::time::Duration;
use std::{env, fmt};

use serde::{Deserialize, Deserializer, Serialize};
use url::Url;

use crate::{AgentError, Result};

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_MODEL: &str = "gpt-4.1-mini";

#[derive(Clone, Serialize, PartialEq)]
pub struct AiConfig {
    base_url: String,
    #[serde(skip_serializing, default)]
    api_key: String,
    model: String,
    temperature: f32,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

#[derive(Deserialize)]
struct RawAiConfig {
    base_url: String,
    #[serde(default)]
    api_key: String,
    model: String,
    temperature: f32,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

impl<'de> Deserialize<'de> for AiConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let raw = RawAiConfig::deserialize(deserializer)?;
        let config = Self {
            base_url: raw.base_url,
            api_key: raw.api_key,
            model: raw.model,
            temperature: raw.temperature,
            timeout_secs: raw.timeout_secs,
        };
        config.validate().map_err(serde::de::Error::custom)?;
        Ok(config)
    }
}

impl fmt::Debug for AiConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AiConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &"[REDACTED]")
            .field("model", &self.model)
            .field("temperature", &self.temperature)
            .field("timeout_secs", &self.timeout_secs)
            .finish()
    }
}

const fn default_timeout_secs() -> u64 {
    120
}

impl AiConfig {
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn temperature(&self) -> f32 {
        self.temperature
    }

    pub fn timeout_secs(&self) -> u64 {
        self.timeout_secs
    }

    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        temperature: f32,
    ) -> Result<Self> {
        let config = Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
            temperature,
            timeout_secs: default_timeout_secs(),
        };
        config.validate()?;
        Ok(config)
    }

    pub fn from_env() -> Result<Self> {
        let temperature = match env::var("BKMSA_TEMPERATURE") {
            Ok(value) => value.parse::<f32>().map_err(|_| {
                AgentError::InvalidConfig("BKMSA_TEMPERATURE must be a number".into())
            })?,
            Err(env::VarError::NotPresent) => 0.2,
            Err(env::VarError::NotUnicode(_)) => {
                return Err(AgentError::InvalidConfig(
                    "BKMSA_TEMPERATURE must be valid Unicode".into(),
                ));
            }
        };
        let timeout_secs = match env::var("BKMSA_TIMEOUT_SECS") {
            Ok(value) => value.parse::<u64>().map_err(|_| {
                AgentError::InvalidConfig("BKMSA_TIMEOUT_SECS must be an integer".into())
            })?,
            Err(env::VarError::NotPresent) => default_timeout_secs(),
            Err(env::VarError::NotUnicode(_)) => {
                return Err(AgentError::InvalidConfig(
                    "BKMSA_TIMEOUT_SECS must be valid Unicode".into(),
                ));
            }
        };
        let config = Self {
            base_url: optional_env("BKMSA_BASE_URL", DEFAULT_BASE_URL)?,
            api_key: match env::var("BKMSA_API_KEY") {
                Ok(value) => value,
                Err(env::VarError::NotPresent) => {
                    return Err(AgentError::MissingConfig("BKMSA_API_KEY"));
                }
                Err(env::VarError::NotUnicode(_)) => {
                    return Err(AgentError::InvalidConfig(
                        "BKMSA_API_KEY must be valid Unicode".into(),
                    ));
                }
            },
            model: optional_env("BKMSA_MODEL", DEFAULT_MODEL)?,
            temperature,
            timeout_secs,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.base_url.len() > 2_048 || self.model.len() > 256 || self.api_key.len() > 16_384 {
            return Err(AgentError::InvalidConfig(
                "base_url, model, or api_key exceeds its length limit".into(),
            ));
        }
        if self.model.chars().any(char::is_control) || self.api_key.chars().any(char::is_control) {
            return Err(AgentError::InvalidConfig(
                "model and api_key must not contain control characters".into(),
            ));
        }
        validate_base_url(&self.base_url)?;
        if self.api_key.trim().is_empty() {
            return Err(AgentError::MissingConfig("BKMSA_API_KEY"));
        }
        if self.model.trim().is_empty() {
            return Err(AgentError::InvalidConfig("model cannot be empty".into()));
        }
        if !(0.0..=2.0).contains(&self.temperature) {
            return Err(AgentError::InvalidConfig(
                "temperature must be between 0 and 2".into(),
            ));
        }
        const MAX_TIMEOUT_SECS: u64 = 15 * 60;
        if !(1..=MAX_TIMEOUT_SECS).contains(&self.timeout_secs) {
            return Err(AgentError::InvalidConfig(format!(
                "timeout_secs must be between 1 and {MAX_TIMEOUT_SECS}"
            )));
        }
        Ok(())
    }

    #[cfg(feature = "native-client")]
    pub(crate) fn endpoint(&self, suffix: &str) -> Result<Url> {
        let base = format!("{}/", self.base_url.trim_end_matches('/'));
        Url::parse(&base)
            .map_err(|error| AgentError::InvalidConfig(format!("invalid base_url: {error}")))?
            .join(suffix.trim_start_matches('/'))
            .map_err(|error| AgentError::InvalidConfig(format!("invalid endpoint path: {error}")))
    }

    #[cfg(feature = "native-client")]
    pub(crate) fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs)
    }
}

fn optional_env(name: &'static str, default: &str) -> Result<String> {
    match env::var(name) {
        Ok(value) => Ok(value),
        Err(env::VarError::NotPresent) => Ok(default.into()),
        Err(env::VarError::NotUnicode(_)) => Err(AgentError::InvalidConfig(format!(
            "{name} must be valid Unicode"
        ))),
    }
}

fn validate_base_url(value: &str) -> Result<()> {
    if value != value.trim() {
        return Err(AgentError::InvalidConfig(
            "base_url must not contain surrounding whitespace".into(),
        ));
    }
    let url = Url::parse(value)
        .map_err(|error| AgentError::InvalidConfig(format!("invalid base_url: {error}")))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(AgentError::InvalidConfig(
            "base_url must be an HTTP(S) URL with a host".into(),
        ));
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AgentError::InvalidConfig(
            "base_url must not contain credentials, a query, or a fragment".into(),
        ));
    }
    if url.scheme() == "http" && !is_loopback_host(url.host_str().unwrap_or_default()) {
        return Err(AgentError::InvalidConfig(
            "base_url must use HTTPS unless it targets loopback".into(),
        ));
    }
    Ok(())
}

fn is_loopback_host(host: &str) -> bool {
    host.parse::<std::net::IpAddr>()
        .is_ok_and(|address| address.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "native-client")]
    #[test]
    fn endpoint_does_not_duplicate_slashes() {
        let config = AiConfig {
            base_url: "https://example.test/v1/".into(),
            api_key: "secret".into(),
            model: "model".into(),
            temperature: 0.2,
            timeout_secs: 10,
        };
        assert_eq!(
            config.endpoint("/chat/completions").unwrap().as_str(),
            "https://example.test/v1/chat/completions"
        );
    }

    #[test]
    fn rejects_unsafe_base_url_shapes() {
        for value in [
            "file:///tmp/provider",
            "https://user:pass@example.test/v1",
            "https://example.test/v1?target=other",
            "https://example.test/v1#fragment",
            " http://example.test/v1",
            "http://example.test/v1",
        ] {
            let config = AiConfig {
                base_url: value.into(),
                api_key: "secret".into(),
                model: "model".into(),
                temperature: 0.2,
                timeout_secs: 10,
            };
            assert!(config.validate().is_err(), "{value}");
        }
    }

    #[test]
    fn permits_loopback_http_for_local_providers() {
        let config = AiConfig {
            base_url: "http://127.0.0.1:11434/v1".into(),
            api_key: "secret".into(),
            model: "model".into(),
            temperature: 0.2,
            timeout_secs: 10,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn secret_is_not_serialized() {
        let config = AiConfig {
            base_url: "https://example.test/v1".into(),
            api_key: "do-not-leak".into(),
            model: "model".into(),
            temperature: 0.2,
            timeout_secs: 10,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("do-not-leak"));
        assert!(!format!("{config:?}").contains("do-not-leak"));
    }
}
