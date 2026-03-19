use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::errors::{CompilerError, CompilerResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BuildLanguage {
    Ts,
    Python,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenClawConfig {
    pub gateway: GatewayConfig,
    pub build: BuildConfig,
    pub runtimes: RuntimeConfig,
    pub llm_providers: Vec<LlmProviderConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayConfig {
    pub url: String,
    pub api_key_env: String,
    #[serde(default)]
    pub executable: Option<PathBuf>,
    #[serde(default)]
    pub cors_origin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildConfig {
    pub source: PathBuf,
    pub language: BuildLanguage,
    pub output_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub sandbox_backend: String,
    pub python_image: String,
    pub node_image: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmProviderConfig {
    pub name: String,
    pub api_key_env: String,
    pub default_model: String,
}

impl OpenClawConfig {
    pub fn template(default_source: impl Into<PathBuf>) -> Self {
        Self {
            gateway: GatewayConfig {
                url: "http://127.0.0.1:8080".to_owned(),
                api_key_env: "CLAW_GATEWAY_API_KEY".to_owned(),
                executable: None,
                cors_origin: None,
            },
            build: BuildConfig {
                source: default_source.into(),
                language: BuildLanguage::Ts,
                output_dir: PathBuf::from("generated/claw"),
            },
            runtimes: RuntimeConfig {
                sandbox_backend: "docker".to_owned(),
                python_image: "python:3.11-slim".to_owned(),
                node_image: "node:22".to_owned(),
            },
            llm_providers: vec![
                LlmProviderConfig {
                    name: "openai".to_owned(),
                    api_key_env: "OPENAI_API_KEY".to_owned(),
                    default_model: "gpt-4o".to_owned(),
                },
                LlmProviderConfig {
                    name: "anthropic".to_owned(),
                    api_key_env: "ANTHROPIC_API_KEY".to_owned(),
                    default_model: "claude-sonnet-4-6".to_owned(),
                },
            ],
        }
    }

    pub fn load(path: &Path) -> CompilerResult<Self> {
        let contents = fs::read_to_string(path).map_err(|source| CompilerError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        serde_json::from_str(&contents).map_err(|source| CompilerError::ParseError {
            message: format!("failed to parse {}: {source}", path.display()),
            span: 0..0,
        })
    }

    pub fn write_pretty(&self, path: &Path) -> CompilerResult<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| CompilerError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let rendered = serde_json::to_string_pretty(self).map_err(|source| CompilerError::ParseError {
            message: format!("failed to serialize {}: {source}", path.display()),
            span: 0..0,
        })?;

        fs::write(path, format!("{rendered}\n")).map_err(|source| CompilerError::Io {
            path: path.to_path_buf(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{BuildLanguage, OpenClawConfig};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn template_contains_gateway_runtimes_and_llm_providers() {
        let config = OpenClawConfig::template("example.claw");
        let rendered = serde_json::to_string_pretty(&config).unwrap();

        assert_eq!(config.build.language, BuildLanguage::Ts);
        assert!(rendered.contains("\"gateway\""));
        assert!(rendered.contains("\"runtimes\""));
        assert!(rendered.contains("\"llm_providers\""));
        assert!(rendered.contains("CLAW_GATEWAY_API_KEY"));
        assert!(rendered.contains("python:3.11-slim"));
        assert!(rendered.contains("\"executable\": null"));
        assert!(rendered.contains("\"cors_origin\": null"));
        assert!(rendered.contains("gpt-4o"));
        assert!(rendered.contains("claude-sonnet-4-6"));
    }

    #[test]
    fn writes_and_reads_config_roundtrip() {
        let config = OpenClawConfig::template("example.claw");
        let path = std::env::temp_dir().join(format!(
            "claw-config-{}.json",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        config.write_pretty(&path).unwrap();
        let loaded = OpenClawConfig::load(&path).unwrap();

        assert_eq!(loaded, config);

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn load_defaults_optional_gateway_fields_when_missing() {
        let path = std::env::temp_dir().join(format!(
            "claw-config-missing-gateway-fields-{}.json",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        fs::write(
            &path,
            r#"{
  "gateway": {
    "url": "http://127.0.0.1:8080",
    "api_key_env": "CLAW_GATEWAY_API_KEY"
  },
  "build": {
    "source": "example.claw",
    "language": "ts",
    "output_dir": "generated/claw"
  },
  "runtimes": {
    "sandbox_backend": "docker",
    "python_image": "python:3.11-slim",
    "node_image": "node:22"
  },
  "llm_providers": []
}
"#,
        )
        .unwrap();

        let loaded = OpenClawConfig::load(&path).unwrap();

        assert_eq!(loaded.gateway.executable, None);
        assert_eq!(loaded.gateway.cors_origin, None);

        fs::remove_file(path).unwrap();
    }
}
