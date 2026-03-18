use std::path::PathBuf;

use thiserror::Error;

use crate::ast::Span;

pub type CompilerResult<T> = Result<T, CompilerError>;

#[derive(Debug, Error)]
pub enum CompilerError {
    #[error("parse error at {span:?}: {message}")]
    ParseError { message: String, span: Span },

    #[error("duplicate symbol `{name}` first defined at {first_span:?}, redefined at {second_span:?}")]
    DuplicateSymbol {
        name: String,
        first_span: Span,
        second_span: Span,
    },

    #[error("undefined tool `{name}` at {span:?}")]
    UndefinedTool { name: String, span: Span },

    #[error("undefined agent `{name}` at {span:?}")]
    UndefinedAgent { name: String, span: Span },

    #[error("undefined client `{name}` at {span:?}")]
    UndefinedClient { name: String, span: Span },

    #[error("undefined type `{name}` at {span:?}")]
    UndefinedType { name: String, span: Span },

    #[error("type mismatch at {span:?}: expected `{expected}`, found `{found}`")]
    TypeMismatch {
        expected: String,
        found: String,
        span: Span,
    },

    #[error("failed to read `{path}`: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl CompilerError {
    pub fn span(&self) -> Option<&Span> {
        match self {
            Self::ParseError { span, .. }
            | Self::UndefinedTool { span, .. }
            | Self::UndefinedAgent { span, .. }
            | Self::UndefinedClient { span, .. }
            | Self::UndefinedType { span, .. }
            | Self::TypeMismatch { span, .. } => Some(span),
            Self::DuplicateSymbol { second_span, .. } => Some(second_span),
            Self::Io { .. } => None,
        }
    }
}
