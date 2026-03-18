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
                api_key_env: "OPENCLAW_GATEWAY_API_KEY".to_owned(),
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
                    default_model: "gpt-5.4".to_owned(),
                },
                LlmProviderConfig {
                    name: "anthropic".to_owned(),
                    api_key_env: "ANTHROPIC_API_KEY".to_owned(),
                    default_model: "claude-sonnet-4-5".to_owned(),
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
        assert!(rendered.contains("OPENCLAW_GATEWAY_API_KEY"));
        assert!(rendered.contains("python:3.11-slim"));
    }

    #[test]
    fn writes_and_reads_config_roundtrip() {
        let config = OpenClawConfig::template("example.claw");
        let path = std::env::temp_dir().join(format!(
            "openclaw-config-{}.json",
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
}
