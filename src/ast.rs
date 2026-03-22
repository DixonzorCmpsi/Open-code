use std::ops::Range;

use serde::Serialize;

// A Span represents the byte range of a token in the original source string.
pub type Span = Range<usize>;

// --- The Root Document ---

#[derive(Debug, Clone, PartialEq, Serialize)]
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
    pub synthesizers: Vec<SynthesizerDecl>,
    pub span: Span,
}

// --- Data Types ---

#[derive(Debug, Clone, PartialEq, Serialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TypeDecl {
    pub name: String,
    pub fields: Vec<TypeField>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TypeField {
    pub name: String,
    pub data_type: DataType,
    pub constraints: Vec<Constraint>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Constraint {
    pub name: String,
    pub value: SpannedExpr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClientDecl {
    pub name: String,
    pub provider: String,
    pub model: String,
    pub retries: Option<u32>,
    pub timeout_ms: Option<u32>,
    pub endpoint: Option<SpannedExpr>,
    pub api_key: Option<SpannedExpr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SynthesizerDecl {
    pub name: String,
    pub client: String,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u64>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolDecl {
    pub name: String,
    pub arguments: Vec<TypeField>,
    pub return_type: Option<DataType>,
    pub invoke_path: Option<String>,
    pub using: Option<UsingExpr>,
    pub synthesizer: Option<String>,
    pub test_block: Option<TestBlock>,
    /// Env var names the synthesized implementation requires (e.g. ["GOOGLE_API_KEY"]).
    /// Declared with `secrets { KEY1 KEY2 }` inside the tool body.
    pub secrets: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum UsingExpr {
    Fetch,
    Playwright,
    Bash,
    Mcp(String),
    Baml(String),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TestBlock {
    pub input: Vec<(String, SpannedExpr)>,
    pub expect: Vec<(String, ExpectOp)>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum ExpectOp {
    NotEmpty,
    Gt(f64),
    Lt(f64),
    Gte(f64),
    Lte(f64),
    Eq(SpannedExpr),
    Matches(String),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AgentDecl {
    pub name: String,
    pub extends: Option<String>,
    pub client: Option<String>,
    pub system_prompt: Option<String>,
    pub tools: Vec<String>,
    pub settings: AgentSettings,
    #[serde(default)]
    pub dynamic_reasoning: std::cell::Cell<bool>,
    pub span: Span,
}

// --- Execution Workflows ---

/// Artifact placement spec — declared inside a workflow to save output to a file.
/// `format` = "json" | "markdown" | "text" | "html"
/// `path`   = file path, supports `~` and `${arg_name}` interpolation
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ArtifactSpec {
    pub format: String,
    pub path: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorkflowDecl {
    pub name: String,
    pub arguments: Vec<TypeField>,
    pub return_type: Option<DataType>,
    pub artifact: Option<ArtifactSpec>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TestDecl {
    pub name: String,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MockDecl {
    pub target_agent: String,
    pub output: Vec<(String, SpannedExpr)>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Block {
    pub statements: Vec<Statement>,
    pub span: Span,
}

// --- Else-If Chaining ---

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum ElseBranch {
    Else(Block),
    ElseIf(Box<Statement>), // Must be Statement::IfCond
}

// --- Statements ---

/// What to do when a `reason {}` block evaluates the synthesized tool output and decides it
/// does not meet the goal. `Retry { max }` re-runs the same code up to `max` times.
/// `ReSynthesize` triggers a new synthesis pass before the next attempt.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum OnFailStrategy {
    Retry { max: u32 },
    ReSynthesize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Statement {
    Reason {
        using_agent: String,
        input: String,
        goal: String,
        output_type: DataType,
        bind: String,
        /// Optional fallback strategy if the LLM judges the output unacceptable.
        on_fail: Option<OnFailStrategy>,
        span: Span,
    },
    LetDecl {
        name: String,
        explicit_type: Option<DataType>,
        value: SpannedExpr,
        span: Span,
    },
    ForLoop {
        item_name: String,
        iterator: SpannedExpr, // Any expression (identifier, member access, call, etc.)
        body: Block,
        span: Span,
    },
    IfCond {
        condition: SpannedExpr,
        if_body: Block,
        else_body: Option<ElseBranch>,
        span: Span,
    },
    ExecuteRun {
        agent_name: String,
        kwargs: Vec<(String, SpannedExpr)>,
        require_type: Option<DataType>,
        span: Span,
    },
    Return {
        value: SpannedExpr,
        span: Span,
    },
    TryCatch {
        try_body: Block,
        catch_name: String,
        catch_type: DataType,
        catch_body: Block,
        span: Span,
    },
    Assert {
        condition: SpannedExpr,
        message: Option<String>,
        span: Span,
    },
    Continue(Span),
    Break(Span),
    Expression(SpannedExpr),
}

// --- Expressions (The lowest level) ---

/// SpannedExpr wraps every expression with its source Span, satisfying §1's
/// "every single node must retain its Span" guarantee.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpannedExpr {
    pub expr: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Expr {
    StringLiteral(String),
    IntLiteral(i64),
    FloatLiteral(f64),
    BoolLiteral(bool),
    Identifier(String),
    ArrayLiteral(Vec<SpannedExpr>),
    Call(String, Vec<SpannedExpr>),
    MemberAccess(Box<SpannedExpr>, String),
    MethodCall(Box<SpannedExpr>, String, Vec<SpannedExpr>),
    ExecuteRun {
        agent_name: String,
        kwargs: Vec<(String, SpannedExpr)>,
        require_type: Option<DataType>,
    },
    /// Direct deterministic tool invocation — `call ToolName(arg: val, ...)`.
    /// Bypasses any LLM; the tool's `invoke:` handler is called directly.
    DirectToolCall {
        tool_name: String,
        args: Vec<(String, SpannedExpr)>,
    },
    BinaryOp {
        left: Box<SpannedExpr>,
        op: BinaryOp,
        right: Box<SpannedExpr>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum BinaryOp {
    Equal,
    NotEqual,
    LessThan,
    GreaterThan,
    LessEq,
    GreaterEq,
}

// The AST spec references these nodes but does not spell them out inline.
// We define them here so the tree stays closed and every declaration remains
// span-carrying from the first compiler milestone onward.

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImportDecl {
    pub names: Vec<String>,
    pub source: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ListenerDecl {
    pub name: String,
    pub event_type: String,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AgentSettings {
    pub entries: Vec<AgentSetting>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AgentSetting {
    pub name: String,
    pub value: SettingValue,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum SettingValue {
    Int(i64),
    Float(f64),
    Boolean(bool),
}

#[cfg(test)]
mod tests {
    use super::{
        AgentDecl, AgentSettings, Block, DataType, Document, Expr, Span, SpannedExpr, Statement,
    };

    /// Helper to wrap an Expr with a Span.
    fn spanned(expr: Expr, span: Span) -> SpannedExpr {
        SpannedExpr { expr, span }
    }

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
                dynamic_reasoning: std::cell::Cell::new(false),
                span: span.clone(),
            }],
            workflows: Vec::new(),
            listeners: Vec::new(),
            tests: Vec::new(),
            mocks: Vec::new(),
            synthesizers: Vec::new(),
            span: span.clone(),
        };

        let workflow_body = Block {
            statements: vec![Statement::Return {
                value: spanned(Expr::Identifier("result".to_owned()), span.clone()),
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

    #[test]
    fn spanned_expr_carries_span() {
        let se = spanned(Expr::IntLiteral(42), 10..12);
        assert_eq!(se.span, (10..12));
        assert_eq!(se.expr, Expr::IntLiteral(42));
    }

    #[test]
    fn else_if_chaining() {
        use super::ElseBranch;
        let span: Span = 0..50;

        let inner_if = Statement::IfCond {
            condition: spanned(Expr::BoolLiteral(true), span.clone()),
            if_body: Block {
                statements: vec![],
                span: span.clone(),
            },
            else_body: None,
            span: span.clone(),
        };

        let outer = Statement::IfCond {
            condition: spanned(Expr::BoolLiteral(false), span.clone()),
            if_body: Block {
                statements: vec![],
                span: span.clone(),
            },
            else_body: Some(ElseBranch::ElseIf(Box::new(inner_if))),
            span: span.clone(),
        };

        if let Statement::IfCond { else_body, .. } = &outer {
            assert!(matches!(else_body, Some(ElseBranch::ElseIf(_))));
        } else {
            panic!("Expected IfCond");
        }
    }

    #[test]
    fn for_loop_with_expr_iterator() {
        let span: Span = 0..30;

        // for (tag in result.tags) { ... }
        let iterator = spanned(
            Expr::MemberAccess(
                Box::new(spanned(Expr::Identifier("result".to_owned()), 15..21)),
                "tags".to_owned(),
            ),
            15..26,
        );

        let for_loop = Statement::ForLoop {
            item_name: "tag".to_owned(),
            iterator,
            body: Block {
                statements: vec![],
                span: span.clone(),
            },
            span: span.clone(),
        };

        if let Statement::ForLoop { iterator, .. } = &for_loop {
            assert!(matches!(iterator.expr, Expr::MemberAccess(_, _)));
        } else {
            panic!("Expected ForLoop");
        }
    }
}
