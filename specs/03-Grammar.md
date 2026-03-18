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

// Imports
import_decl = { "import" ~ "{" ~ identifier ~ ("," ~ identifier)* ~ "}" ~ "from" ~ string_lit }

// Types (With optional Semantic Constraints)
type_decl = { "type" ~ identifier ~ "{" ~ type_field+ ~ "}" }
type_field = { identifier ~ ":" ~ data_type ~ constraint_block? }
constraint_block = { "@" ~ identifier ~ "(" ~ (string_lit | number_lit) ~ ")" }

// Clients
client_decl = { "client" ~ identifier ~ "{" ~ client_setting+ ~ "}" }
client_setting = { identifier ~ "=" ~ (string_lit | number_lit) }

// Tools
tool_decl = { "tool" ~ identifier ~ "(" ~ tool_args? ~ ")" ~ ("->" ~ data_type)? ~ block? }
tool_args = { type_field ~ ("," ~ type_field)* }

// Agents
agent_decl = { "agent" ~ identifier ~ ("extends" ~ identifier)? ~ "{" ~ agent_prop+ ~ "}" }
agent_prop = { 
    ("client" ~ "=" ~ identifier) |
    ("system_prompt" ~ "=" ~ string_lit) |
    ("tools" ~ ("=" | "+=") ~ "[" ~ identifier ~ ("," ~ identifier)* ~ "]") |
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
    execute_stmt |
    return_stmt |
    expr
}

// Control Flow
let_stmt = { "let" ~ identifier ~ (":" ~ data_type)? ~ "=" ~ expr }
for_stmt = { "for" ~ "(" ~ identifier ~ "in" ~ identifier ~ ")" ~ block }
if_stmt = { "if" ~ "(" ~ condition ~ ")" ~ block ~ ("else" ~ block)? }
return_stmt = { "return" ~ expr }

// Execution primitive
execute_stmt = { 
    "execute" ~ identifier ~ ".run" ~ "(" ~ 
    execute_kwargs ~ 
    ")" 
}
execute_kwargs = { (identifier ~ ":" ~ expr ~ ","?)+ }

// Event Listeners
listener_decl = { "listener" ~ identifier ~ "(" ~ "event" ~ ":" ~ identifier ~ ")" ~ block }

// Tests and Mocks
test_decl = { "test" ~ string_lit ~ block }
mock_decl = { "mock" ~ identifier ~ "(" ~ expr ~ ")" ~ "->" ~ expr }```

## 2. Key Deviations from General Purpose Languages
- **No Class Methods:** Agents are data structures, not active classes. The execution of an agent happens via the `execute AgentName.run(...)` primitive.
- **Strict Keyword Boundaries:** `tools` and `settings` are protected keywords inside the `agent` declaration.
- **Explicit Type Enforcement:** Unlike Python, all function/workflow inputs and `let` variable definitions map to an explicit struct.
