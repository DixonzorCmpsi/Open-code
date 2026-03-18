# OpenClaw DSL: Formal Grammar (PEG)

This document defines the formal Parsing Expression Grammar (PEG) that the `clawc` lexer and parser will use (via `pest` or `winnow`).

## 1. Syntax Rules

The grammar is designed to be C/Rust/TypeScript-like, utilizing curly braces for block scoping and explicit type assignments.

```peg
// --- Core Document Structure ---
document = _{ SOI ~ (import_decl | type_decl | client_decl | tool_decl | agent_decl | workflow_decl | listener_decl | test_decl | mock_decl)* ~ EOI }

// --- Primitives ---
WHITESPACE = _{ " " | "\t" | "\r" | "\n" }
COMMENT = _{ "//" ~ (!"\n" ~ ANY)* ~ "\n" }

identifier = @{ ASCII_ALPHA ~ (ASCII_ALNUM | "_")* }
tool_ref = @{ identifier ~ ("." ~ identifier)? }  // Supports dotted refs: Browser.search, FileSystem.write
string_lit = @{ "\"" ~ (!"\"" ~ ANY)* ~ "\"" }
number_lit = @{ "-"? ~ ASCII_DIGIT+ ~ ("." ~ ASCII_DIGIT+)? }
boolean_lit = { "true" | "false" }

// --- Types ---
data_type = { 
    "string" | "int" | "float" | "boolean" | 
    ("list" ~ "<" ~ identifier ~ ">") | 
    identifier // Custom type reference
}

// --- Declarations ---

// Imports (Phase 7 — parsed into AST but no module resolution exists; imported names
// are resolved by the gateway at runtime, not by the compiler)
import_decl = { "import" ~ "{" ~ identifier ~ ("," ~ identifier)* ~ "}" ~ "from" ~ string_lit }

// Types (With optional Semantic Constraints)
type_decl = { "type" ~ identifier ~ "{" ~ type_field+ ~ "}" }
type_field = { identifier ~ ":" ~ data_type ~ constraint_block? }
constraint_block = { "@" ~ identifier ~ "(" ~ (string_lit | number_lit) ~ ")" }

// Clients
client_decl = { "client" ~ identifier ~ "{" ~ client_setting+ ~ "}" }
client_setting_key = { "provider" | "model" | "retries" | "timeout" | "endpoint" | "api_key" }
client_setting = { client_setting_key ~ "=" ~ expr }

// Tools
tool_decl = { "tool" ~ identifier ~ "(" ~ tool_args? ~ ")" ~ ("->" ~ data_type)? ~ block? }
tool_args = { type_field ~ ("," ~ type_field)* }

// Agents
agent_decl = { "agent" ~ identifier ~ ("extends" ~ identifier)? ~ "{" ~ agent_prop+ ~ "}" }
agent_prop = { 
    ("client" ~ "=" ~ identifier) |
    ("system_prompt" ~ "=" ~ string_lit) |
    ("tools" ~ ("=" | "+=") ~ "[" ~ tool_ref ~ ("," ~ tool_ref)* ~ "]") |
    ("settings" ~ "=" ~ settings_block)
}
settings_block = { "{" ~ (identifier ~ ":" ~ (number_lit | boolean_lit) ~ ","?)+ ~ "}" }

// Workflows (Execution Logic)
workflow_decl = { "workflow" ~ identifier ~ "(" ~ tool_args? ~ ")" ~ ("->" ~ data_type)? ~ block }
block = { "{" ~ statement* ~ "}" }

statement = {
    let_stmt |
    for_stmt |
    if_stmt |
    try_stmt |
    execute_stmt |
    return_stmt |
    continue_stmt |
    break_stmt |
    assert_stmt |
    expr
}

// Control Flow
let_stmt = { "let" ~ identifier ~ (":" ~ data_type)? ~ "=" ~ expr }
for_stmt = { "for" ~ "(" ~ identifier ~ "in" ~ expr ~ ")" ~ block }
if_stmt = { "if" ~ "(" ~ condition ~ ")" ~ block ~ ("else" ~ (if_stmt | block))? }
try_stmt = { "try" ~ block ~ "catch" ~ "(" ~ identifier ~ ":" ~ data_type ~ ")" ~ block }
return_stmt = { "return" ~ expr }
continue_stmt = { "continue" }
break_stmt = { "break" }

// Execution primitive
execute_stmt = {
    "execute" ~ identifier ~ ".run" ~ "(" ~
    execute_kwargs ~
    ")"
}
execute_kwargs = { (identifier ~ ":" ~ expr ~ ","?)+ }

// Assert statement (test blocks only)
assert_stmt = { "assert" ~ expr ~ ("," ~ string_lit)? }

// Expressions (extended)
expr = {
    call_expr |
    binary_expr |
    method_call_expr |
    member_access_expr |
    array_literal |
    string_lit |
    number_lit |
    boolean_lit |
    identifier
}
member_access_expr = { expr ~ "." ~ identifier }  // Field access: result.tags
call_expr = { identifier ~ "(" ~ (expr ~ ("," ~ expr)*)? ~ ")" }  // Nested workflow calls
method_call_expr = { expr ~ "." ~ identifier ~ "(" ~ (expr ~ ("," ~ expr)*)? ~ ")" }
binary_expr = { expr ~ binary_op ~ expr }
binary_op = { "==" | "!=" | "<=" | ">=" | "<" | ">" }
array_literal = { "[" ~ (expr ~ ("," ~ expr)*)? ~ "]" }
condition = { expr }  // Condition is any expression that evaluates to boolean

// Event Listeners (Phase 7 — parsed into AST but NOT compiled or executed)
listener_decl = { "listener" ~ identifier ~ "(" ~ "event" ~ ":" ~ identifier ~ ")" ~ block }

// Tests and Mocks
test_decl = { "test" ~ string_lit ~ block }
mock_decl = { "mock" ~ identifier ~ "{" ~ (identifier ~ ":" ~ expr ~ ","?)+ ~ "}" }```

## 2. Key Deviations from General Purpose Languages
- **No Class Methods:** Agents are data structures, not active classes. The execution of an agent happens via the `execute AgentName.run(...)` primitive.
- **Strict Keyword Boundaries:** `tools` and `settings` are protected keywords inside the `agent` declaration.
- **Explicit Type Enforcement:** Unlike Python, all function/workflow inputs and `let` variable definitions map to an explicit struct.
