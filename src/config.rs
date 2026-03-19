use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::errors::{CompilerError, CompilerResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BuildLanguage {
    Opencode,
    Ts,
    Python,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClawConfig {
    pub build: BuildConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildConfig {
    pub source: PathBuf,
    pub language: BuildLanguage,
    pub output_dir: PathBuf,
}

impl ClawConfig {
    pub fn template(default_source: impl Into<PathBuf>) -> Self {
        Self {
            build: BuildConfig {
                source: default_source.into(),
                language: BuildLanguage::Opencode,
                output_dir: PathBuf::from("generated"),
            },
        }
    }

    pub fn load(path: &Path) -> CompilerResult<Self> {
        let contents = fs::read_to_string(path).map_err(|source| CompilerError::IoError {
            message: format!("failed to read {}: {}", path.display(), source),
            span: 0..0,
        })?;

        serde_json::from_str(&contents).map_err(|source| CompilerError::ParseError {
            message: format!("failed to parse {}: {source}", path.display()),
            span: 0..0,
        })
    }

    pub fn write_pretty(&self, path: &Path) -> CompilerResult<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| CompilerError::IoError {
                message: format!("failed to create directory {}: {}", parent.display(), source),
                span: 0..0,
            })?;
        }

        let rendered = serde_json::to_string_pretty(self).map_err(|source| CompilerError::ParseError {
            message: format!("failed to serialize {}: {source}", path.display()),
            span: 0..0,
        })?;

        fs::write(path, format!("{rendered}\n")).map_err(|source| CompilerError::IoError {
            message: format!("failed to write {}: {}", path.display(), source),
            span: 0..0,
        })
    }
}
