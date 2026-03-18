# OpenClaw DSL: Rust AST Specifications

This document defines exactly how the `03-Grammar.md` parsing rules are lowered into heavily-typed Rust data structures (the Abstract Syntax Tree) by the `clawc` compiler.

## 1. Core Principles for the AST
* **Span Tracking:** Every single node in the AST must retain its `Span` (the start and end byte index of the original file). This is non-negotiable for producing world-class compiler errors.
* **Immutability:** The parsed AST is an immutable read-only tree. Validation passes generate entirely new IrTrees rather than mutating the parsed AST.

## 2. The Abstract Syntax Tree (`src/ast.rs`)

```rust
use std::ops::Range;

// A Span represents the byte range of a token in the original source string
pub type Span = Range<usize>;

// --- The Root Document ---

#[derive(Debug, Clone)]
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
    List(Box<DataType>, Span),    // e.g., list<string>
    Custom(String, Span),         // e.g., SearchResult
}

// --- High-Level Declarations ---

#[derive(Debug, Clone)]
pub struct TypeDecl {
    pub name: String,
    pub fields: Vec<TypeField>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TypeField {
    pub name: String,
    pub data_type: DataType,
    pub constraints: Vec<Constraint>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Constraint {
    pub name: String,
    pub value: SpannedExpr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ClientDecl {
    pub name: String,
    pub provider: String,
    pub model: String,
    pub retries: Option<u32>,
    pub timeout_ms: Option<u32>,
    pub endpoint: Option<SpannedExpr>,  // Custom LLM endpoint, e.g., env("CUSTOM_LLM_URL")
    pub api_key: Option<SpannedExpr>,   // API key source, e.g., env("CUSTOM_LLM_KEY")
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ToolDecl {
    pub name: String,
    pub arguments: Vec<TypeField>,
    pub return_type: Option<DataType>,
    pub invoke_path: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct AgentDecl {
    pub name: String,
    pub extends: Option<String>,
    pub client: Option<String>,
    pub system_prompt: Option<String>,
    pub tools: Vec<String>,
    pub settings: AgentSettings,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct AgentSettings {
    pub entries: Vec<AgentSetting>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct AgentSetting {
    pub name: String,           // e.g., "max_steps", "temperature"
    pub value: SettingValue,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum SettingValue {
    Int(i64),
    Float(f64),
    Boolean(bool),
}

// --- Execution Workflows ---

#[derive(Debug, Clone)]
pub struct WorkflowDecl {
    pub name: String,
    pub arguments: Vec<TypeField>,
    pub return_type: Option<DataType>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TestDecl {
    pub name: String,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct MockDecl {
    pub target_agent: String,
    pub output: Vec<(String, SpannedExpr)>,  // Key-value object literal (Phase 6C migration)
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub statements: Vec<Statement>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Statement {
    LetDecl {
        name: String,
        explicit_type: Option<DataType>,
        value: SpannedExpr,
        span: Span,
    },
    ForLoop {
        item_name: String,
        iterator: SpannedExpr,  // Any expression (identifier, member access, call, etc.)
        body: Block,
        span: Span,
    },
    IfCond {
        condition: SpannedExpr,
        if_body: Block,
        else_body: Option<ElseBranch>,  // Supports else-if chaining
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
        catch_type: DataType,  // Required — OpenClaw has no untyped bindings
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

// --- Else-If Chaining ---

// ElseBranch supports both plain `else { }` and chained `else if (...) { } else { }`.
// `else if` is syntactic sugar — the parser desugars it into nested IfCond nodes:
//   if (a) { X } else if (b) { Y } else { Z }
//   → IfCond { condition: a, if_body: X, else_body: ElseIf(IfCond { condition: b, ..., else_body: Else(Z) }) }
#[derive(Debug, Clone)]
pub enum ElseBranch {
    Else(Block),
    ElseIf(Box<Statement>),  // Must be Statement::IfCond
}

// --- Expressions (The lowest level) ---

// SpannedExpr wraps every expression with its source Span, satisfying §1's
// "every single node must retain its Span" guarantee. All references to Expr
// in Statement variants, kwargs, etc. MUST use SpannedExpr.
#[derive(Debug, Clone)]
pub struct SpannedExpr {
    pub expr: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Expr {
    StringLiteral(String),
    IntLiteral(i64),
    FloatLiteral(f64),
    BoolLiteral(bool),
    Identifier(String),
    ArrayLiteral(Vec<SpannedExpr>),
    Call(String, Vec<SpannedExpr>),                    // Nested workflow call: InnerWorkflow(args)
    MemberAccess(Box<SpannedExpr>, String),           // Field access: result.tags
    MethodCall(Box<SpannedExpr>, String, Vec<SpannedExpr>),  // e.g., reports.append(report)
    BinaryOp {
        left: Box<SpannedExpr>,
        op: BinaryOp,
        right: Box<SpannedExpr>,
    },
    ExecuteRun {                               // Inline agent execution as expression
        agent_name: String,
        kwargs: Vec<(String, SpannedExpr)>,
        require_type: Option<DataType>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    Equal,      // ==
    NotEqual,   // !=
    LessThan,   // <
    GreaterThan,// >
    LessEq,     // <=
    GreaterEq,  // >=
}
```

## 3. Next Steps
Once the `winnow` parser spits out this `Document` struct, it is passed directly into the **Semantic Analyzer** (Phase 2), which consumes this tree and performs static `05-Type-System.md` guarantees (checking if the `agent_name` actually exists in the `agents` vector).
