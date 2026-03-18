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
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ClientDecl {
    pub name: String,
    pub provider: String,
    pub model: String,
    pub retries: Option<u32>,
    pub timeout_ms: Option<u32>,
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
    pub settings: AgentSettings, // max_steps, temperature, etc.
    pub span: Span,
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
    pub mock_input: Expr,
    pub mock_output: Expr,
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

#[derive(Debug, Clone)]
pub enum Expr {
    StringLiteral(String),
    IntLiteral(i64),
    FloatLiteral(f64),
    BoolLiteral(bool),
    Identifier(String),
    ArrayLiteral(Vec<Expr>),
    MethodCall(Box<Expr>, String, Vec<Expr>), // e.g., reports.append(report)
}
```

## 3. Next Steps
Once the `winnow` parser spits out this `Document` struct, it is passed directly into the **Semantic Analyzer** (Phase 2), which consumes this tree and performs static `05-Type-System.md` guarantees (checking if the `agent_name` actually exists in the `agents` vector).
