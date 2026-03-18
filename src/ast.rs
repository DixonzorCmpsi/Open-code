use std::ops::Range;

// A Span represents the byte range of a token in the original source string.
pub type Span = Range<usize>;

// --- The Root Document ---

#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub imports: Vec<ImportDecl>,
    pub types: Vec<TypeDecl>,
    pub clients: Vec<ClientDecl>,
    pub tools: Vec<ToolDecl>,
    pub agents: Vec<AgentDecl>,
    pub workflows: Vec<WorkflowDecl>,
    pub listeners: Vec<ListenerDecl>,
    pub tests: Vec<TestDecl>,
    pub mocks: Vec<MockDecl>,
    pub span: Span,
}

// --- Data Types ---

#[derive(Debug, Clone, PartialEq)]
pub enum DataType {
    String(Span),
    Int(Span),
    Float(Span),
    Boolean(Span),
    List(Box<DataType>, Span),
    Custom(String, Span),
}

impl DataType {
    pub fn span(&self) -> &Span {
        match self {
            Self::String(span)
            | Self::Int(span)
            | Self::Float(span)
            | Self::Boolean(span)
            | Self::List(_, span)
            | Self::Custom(_, span) => span,
        }
    }
}

// --- High-Level Declarations ---

#[derive(Debug, Clone, PartialEq)]
pub struct TypeDecl {
    pub name: String,
    pub fields: Vec<TypeField>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeField {
    pub name: String,
    pub data_type: DataType,
    pub constraints: Vec<Constraint>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Constraint {
    pub name: String,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClientDecl {
    pub name: String,
    pub provider: String,
    pub model: String,
    pub retries: Option<u32>,
    pub timeout_ms: Option<u32>,
    pub endpoint: Option<Expr>,
    pub api_key: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolDecl {
    pub name: String,
    pub arguments: Vec<TypeField>,
    pub return_type: Option<DataType>,
    pub invoke_path: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentDecl {
    pub name: String,
    pub extends: Option<String>,
    pub client: Option<String>,
    pub system_prompt: Option<String>,
    pub tools: Vec<String>,
    pub settings: AgentSettings,
    pub span: Span,
}

// --- Execution Workflows ---

#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowDecl {
    pub name: String,
    pub arguments: Vec<TypeField>,
    pub return_type: Option<DataType>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TestDecl {
    pub name: String,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MockDecl {
    pub target_agent: String,
    pub mock_input: Expr,
    pub mock_output: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub statements: Vec<Statement>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    LetDecl {
        name: String,
        explicit_type: Option<DataType>,
        value: Expr,
        span: Span,
    },
    ForLoop {
        item_name: String,
        iterator_name: String,
        body: Block,
        span: Span,
    },
    IfCond {
        condition: Expr,
        if_body: Block,
        else_body: Option<Block>,
        span: Span,
    },
    ExecuteRun {
        agent_name: String,
        kwargs: Vec<(String, Expr)>,
        require_type: Option<DataType>,
        span: Span,
    },
    Return {
        value: Expr,
        span: Span,
    },
    Expression(Expr, Span),
}

// --- Expressions (The lowest level) ---

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    StringLiteral(String),
    IntLiteral(i64),
    FloatLiteral(f64),
    BoolLiteral(bool),
    Identifier(String),
    ArrayLiteral(Vec<Expr>),
    Call(String, Vec<Expr>),
    MethodCall(Box<Expr>, String, Vec<Expr>),
    ExecuteRun {
        agent_name: String,
        kwargs: Vec<(String, Expr)>,
        require_type: Option<DataType>,
    },
    BinaryOp {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinaryOp {
    Equal,
}

// The AST spec references these nodes but does not spell them out inline.
// We define them here so the tree stays closed and every declaration remains
// span-carrying from the first compiler milestone onward.

#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    pub names: Vec<String>,
    pub source: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListenerDecl {
    pub name: String,
    pub event_type: String,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentSettings {
    pub entries: Vec<AgentSetting>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentSetting {
    pub name: String,
    pub value: SettingValue,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SettingValue {
    Int(i64),
    Float(f64),
    Boolean(bool),
}

#[cfg(test)]
mod tests {
    use super::{AgentDecl, AgentSettings, Block, DataType, Document, Expr, Span, Statement};

    #[test]
    fn document_tracks_spans_and_nested_nodes() {
        let span: Span = 0..12;

        let document = Document {
            imports: Vec::new(),
            types: Vec::new(),
            clients: Vec::new(),
            tools: Vec::new(),
            agents: vec![AgentDecl {
                name: "Researcher".to_owned(),
                extends: None,
                client: Some("FastOpenAI".to_owned()),
                system_prompt: Some("Stay deterministic.".to_owned()),
                tools: vec!["WebScraper".to_owned()],
                settings: AgentSettings {
                    entries: Vec::new(),
                    span: span.clone(),
                },
                span: span.clone(),
            }],
            workflows: Vec::new(),
            listeners: Vec::new(),
            tests: Vec::new(),
            mocks: Vec::new(),
            span: span.clone(),
        };

        let workflow_body = Block {
            statements: vec![Statement::Return {
                value: Expr::Identifier("result".to_owned()),
                span: span.clone(),
            }],
            span: span.clone(),
        };

        assert_eq!(document.span, span);
        assert_eq!(document.agents[0].name, "Researcher");
        assert_eq!(
            document.agents[0].settings.span,
            document.agents[0].span
        );
        assert_eq!(DataType::String(0..6).span(), &(0..6));
        assert_eq!(workflow_body.statements.len(), 1);
    }
}
