use thiserror::Error;
use crate::ast::Span;

pub type CompilerResult<T> = Result<T, CompilerError>;

#[derive(Debug, Error, Clone)]
pub enum CompilerError {
    #[error("parse error: {message}")]
    ParseError { message: String, span: Span },

    #[error("undefined tool: {name}")]
    UndefinedTool { name: String, span: Span },

    #[error("undefined agent: {name}")]
    UndefinedAgent { name: String, span: Span },

    #[error("undefined client: {name}")]
    UndefinedClient { name: String, span: Span },

    #[error("undefined type: {name}")]
    UndefinedType { name: String, span: Span },

    #[error("type mismatch: {expected} != {found}")]
    TypeMismatch { expected: String, found: String, span: Span },

    #[error("duplicate declaration: {message}")]
    DuplicateDeclaration { message: String, span: Span },

    #[error("cyclic dependency: {message}")]
    CyclicDependency { message: String, span: Span },

    #[error("codegen error: {message}")]
    CodegenError { message: String, span: Span },

    #[error("I/O error: {message}")]
    IoError { message: String, span: Span },

    #[error("missing return: {workflow_name}")]
    MissingReturn { workflow_name: String, span: Span },

    #[error("invalid control flow: {keyword}")]
    InvalidControlFlow { keyword: String, span: Span },

    #[error("invalid assert: only allowed in test blocks")]
    InvalidAssertOutsideTest { span: Span },

    #[error("unsupported constraint: {name}")]
    UnsupportedConstraint { name: String, span: Span },

    #[error("invalid constraint value for {name}: expected {expected}")]
    InvalidConstraintValue { name: String, expected: String, span: Span },

    #[error("BAML signature conflict: {message}")]
    BamlSignatureConflict { message: String, span: Span },

    #[error("circular type: {type_name} forms a cycle: {cycle_path:?}")]
    CircularType { type_name: String, cycle_path: Vec<String>, span: Span },

    #[error("circular agent extends: {agent_name}")]
    CircularAgentExtends { agent_name: String, span: Span },
}

impl CompilerError {
    pub fn span(&self) -> Option<&Span> {
        match self {
            Self::ParseError { span, .. }
            | Self::UndefinedTool { span, .. }
            | Self::UndefinedAgent { span, .. }
            | Self::UndefinedClient { span, .. }
            | Self::UndefinedType { span, .. }
            | Self::TypeMismatch { span, .. }
            | Self::DuplicateDeclaration { span, .. }
            | Self::CyclicDependency { span, .. }
            | Self::CodegenError { span, .. }
            | Self::IoError { span, .. }
            | Self::MissingReturn { span, .. }
            | Self::InvalidControlFlow { span, .. }
            | Self::InvalidAssertOutsideTest { span, .. }
            | Self::UnsupportedConstraint { span, .. }
            | Self::InvalidConstraintValue { span, .. }
            | Self::BamlSignatureConflict { span, .. }
            | Self::CircularType { span, .. }
            | Self::CircularAgentExtends { span, .. } => Some(span),
        }
    }
}
