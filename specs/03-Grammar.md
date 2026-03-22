# Claw DSL: Formal Grammar (PEG)

This document defines the formal Parsing Expression Grammar (PEG) that the `clawc` lexer and parser implement (via `winnow`).

**Changelog:**

- v0.1 — Initial grammar
- v0.2 — Added synthesis pipeline constructs: `tool_prop`, `using_expr`, `synthesizer_decl`, `reason_stmt`, `on_fail_strategy`, `artifact_decl` (Specs 32–36, 42–48)
- v0.3 — Added `applescript` and `computer` to `using_expr`; added `client:` tool property for `using: computer` (Spec 49)

**Read before touching:** `src/parser.rs`, `src/ast.rs`

---

## 1. Syntax Rules

The grammar is C/Rust/TypeScript-like: curly braces for block scoping, explicit types everywhere.

```peg
// --- Core Document Structure ---
document = _{
    SOI ~
    ( import_decl
    | type_decl
    | client_decl
    | tool_decl
    | agent_decl
    | workflow_decl
    | synthesizer_decl
    | listener_decl
    | test_decl
    | mock_decl
    )* ~
    EOI
}

// --- Primitives ---
WHITESPACE = _{ " " | "\t" | "\r" | "\n" }
COMMENT    = _{ "//" ~ (!"\n" ~ ANY)* ~ "\n" }

identifier  = @{ ASCII_ALPHA ~ (ASCII_ALNUM | "_")* }
tool_ref    = @{ identifier ~ ("." ~ identifier)? }
string_lit  = @{ "\"" ~ (!"\"" ~ ANY)* ~ "\"" }
number_lit  = @{ "-"? ~ ASCII_DIGIT+ ~ ("." ~ ASCII_DIGIT+)? }
boolean_lit = { "true" | "false" }

// --- Types ---
data_type = {
    "string" | "int" | "float" | "boolean" |
    ("list" ~ "<" ~ identifier ~ ">") |
    identifier   // custom type reference
}

// --- Declarations ---

// Imports (parsed into AST; no module resolution at compile time)
import_decl = { "import" ~ "{" ~ identifier ~ ("," ~ identifier)* ~ "}" ~ "from" ~ string_lit }

// Types with optional semantic constraints
type_decl        = { "type" ~ identifier ~ "{" ~ type_field+ ~ "}" }
type_field       = { identifier ~ ":" ~ data_type ~ constraint_block? }
constraint_block = { "@" ~ identifier ~ "(" ~ (string_lit | number_lit) ~ ")" }

// Clients
client_decl        = { "client" ~ identifier ~ "{" ~ client_setting+ ~ "}" }
client_setting_key = { "provider" | "model" | "retries" | "timeout" | "endpoint" | "api_key" }
client_setting     = { client_setting_key ~ "=" ~ expr }

// ─── Tools ─────────────────────────────────────────────────────────────────
//
// A tool body is a set of named properties, not arbitrary statements.
// `invoke:` and `using:` are mutually exclusive (semantic rule, not syntactic).

tool_decl = { "tool" ~ identifier ~ "(" ~ tool_args? ~ ")" ~ ("->" ~ data_type)? ~ ("{" ~ tool_prop* ~ "}")? }
tool_args = { type_field ~ ("," ~ type_field)* }

tool_prop = {
    tool_prop_invoke      |   // invoke: module("path").function("fn")
    tool_prop_using       |   // using: fetch | bash | applescript | computer | playwright | mcp(...) | baml(...)
    tool_prop_client      |   // client: ClientName  (only valid with using: computer)
    tool_prop_synthesizer |   // synthesizer: SynthName
    tool_prop_description |   // description: "..."
    tool_prop_secrets     |   // secrets { ENV_VAR1 ENV_VAR2 }
    tool_prop_test        |   // test { input: {...} expect: {...} }
    tool_prop_examples        // examples { { input: {...} output: {...} } ... }
}

tool_prop_invoke      = { "invoke"      ~ ":" ~ invoke_expr }
tool_prop_using       = { "using"       ~ ":" ~ using_expr }
tool_prop_client      = { "client"      ~ ":" ~ identifier }
tool_prop_synthesizer = { "synthesizer" ~ ":" ~ identifier }
tool_prop_description = { "description" ~ ":" ~ string_lit }
tool_prop_secrets     = { "secrets" ~ "{" ~ identifier+ ~ "}" }
tool_prop_test        = { "test" ~ "{" ~ test_input_block ~ test_expect_block ~ "}" }
tool_prop_examples    = { "examples" ~ "{" ~ example_entry+ ~ "}" }

invoke_expr = {
    ("module" ~ "(" ~ string_lit ~ ")" ~ "." ~ "function" ~ "(" ~ string_lit ~ ")") |
    ("baml"   ~ "(" ~ string_lit ~ ")")
}

using_expr = {
    "fetch"        |
    "playwright"   |
    "bash"         |
    "applescript"  |   // macOS GUI automation via osascript (Spec 49 §1)
    "computer"     |   // vision model + input simulation (Spec 49 §2)
    ("mcp"  ~ "(" ~ string_lit ~ ")") |
    ("baml" ~ "(" ~ string_lit ~ ")")
}

test_input_block  = { "input"  ~ ":" ~ "{" ~ (identifier ~ ":" ~ expr ~ ","?)+ ~ "}" }
test_expect_block = { "expect" ~ ":" ~ "{" ~ (identifier ~ ":" ~ expect_op ~ ","?)+ ~ "}" }
expect_op         = { "!empty" | (compare_op ~ number_lit) | string_lit }
compare_op        = { ">=" | "<=" | ">" | "<" | "==" | "matches" }

example_entry = {
    "{" ~
    "input"  ~ ":" ~ "{" ~ (identifier ~ ":" ~ expr ~ ","?)+ ~ "}" ~ ","? ~
    "output" ~ ":" ~ "{" ~ (identifier ~ ":" ~ expr ~ ","?)+ ~ "}" ~
    "}"
}

// ─── Agents ────────────────────────────────────────────────────────────────

agent_decl = { "agent" ~ identifier ~ ("extends" ~ identifier)? ~ "{" ~ agent_prop+ ~ "}" }
agent_prop = {
    ("client"        ~ "=" ~ identifier)                                              |
    ("system_prompt" ~ "=" ~ string_lit)                                              |
    ("tools"         ~ ("=" | "+=") ~ "[" ~ tool_ref ~ ("," ~ tool_ref)* ~ "]")      |
    ("settings"      ~ "=" ~ settings_block)
}
settings_block = { "{" ~ (identifier ~ ":" ~ (number_lit | boolean_lit) ~ ","?)+ ~ "}" }

// ─── Synthesizer ───────────────────────────────────────────────────────────
//
// Declares the LLM configuration for the synthesis pass (Spec 32–36).
// `retry {}` controls the synthesis repair loop (Spec 36).
// `sandbox_gate {}` controls Stage 2.5 isolation (Spec 49 §3).

synthesizer_decl = { "synthesizer" ~ identifier ~ "{" ~ synthesizer_prop+ ~ "}" }
synthesizer_prop = {
    ("client"      ~ "=" ~ identifier)   |
    ("temperature" ~ "=" ~ number_lit)   |
    ("max_tokens"  ~ "=" ~ number_lit)   |
    synthesizer_retry_block              |
    synthesizer_sandbox_gate_block
}

synthesizer_retry_block = {
    "retry" ~ "{" ~ retry_prop+ ~ "}"
}
retry_prop = {
    ("max_attempts"         ~ ":" ~ number_lit)  |
    ("strategy"             ~ ":" ~ retry_strategy) |
    ("compile_repair_limit" ~ ":" ~ number_lit)  |
    ("on_stuck"             ~ ":" ~ retry_strategy) |
    ("budget_usd"           ~ ":" ~ number_lit)
}
retry_strategy = { "repair" | "rewrite" | "repair_then_rewrite" }

synthesizer_sandbox_gate_block = {
    "sandbox_gate" ~ "{" ~ sandbox_gate_prop+ ~ "}"
}
sandbox_gate_prop = {
    ("enabled"    ~ ":" ~ boolean_lit) |
    ("timeout_ms" ~ ":" ~ number_lit)  |
    ("network"    ~ ":" ~ ("none" | "host"))
}

// ─── Workflows ─────────────────────────────────────────────────────────────

workflow_decl = { "workflow" ~ identifier ~ "(" ~ tool_args? ~ ")" ~ ("->" ~ data_type)? ~ block }
block         = { "{" ~ statement* ~ "}" }

statement = {
    let_stmt      |
    for_stmt      |
    if_stmt       |
    try_stmt      |
    reason_stmt   |   // LLM reasoning block (Spec 32 §5)
    execute_stmt  |
    return_stmt   |
    continue_stmt |
    break_stmt    |
    assert_stmt   |
    expr
}

// Control Flow
let_stmt      = { "let" ~ identifier ~ (":" ~ data_type)? ~ "=" ~ expr }
for_stmt      = { "for" ~ "(" ~ identifier ~ "in" ~ expr ~ ")" ~ block }
if_stmt       = { "if" ~ "(" ~ condition ~ ")" ~ block ~ ("else" ~ (if_stmt | block))? }
try_stmt      = { "try" ~ block ~ "catch" ~ "(" ~ identifier ~ ":" ~ data_type ~ ")" ~ block }
return_stmt   = { "return" ~ expr }
continue_stmt = { "continue" }
break_stmt    = { "break" }

// Execution primitive
execute_stmt  = { "execute" ~ identifier ~ ".run" ~ "(" ~ execute_kwargs ~ ")" }
execute_kwargs = { (identifier ~ ":" ~ expr ~ ","?)+ }

// Reason block — calls LLM at workflow runtime (Spec 32 §5)
reason_stmt = {
    "reason" ~ "{" ~
    "using"    ~ ":" ~ identifier ~ ","? ~   // agent name
    "input"    ~ ":" ~ expr ~ ","?        ~  // input expression
    "goal"     ~ ":" ~ string_lit ~ ","?  ~  // natural-language goal
    on_fail_clause?                          // optional failure strategy
    "}"
}
on_fail_clause   = { "on_fail" ~ ":" ~ on_fail_strategy }
on_fail_strategy = { ("retry" ~ "(" ~ "max" ~ ":" ~ number_lit ~ ")") | "re_synthesize" | "fail" }

// Assert statement (only valid inside test{} blocks)
assert_stmt = { "assert" ~ expr ~ ("," ~ string_lit)? }

// Artifact declaration (inside workflow body — declares output persistence)
artifact_decl = {
    "artifact" ~ "{" ~
    ("format" ~ "=" ~ string_lit ~ ","?) ~
    ("path"   ~ "=" ~ string_lit ~ ","?) ~
    "}"
}

// Expressions
expr = {
    call_expr          |
    binary_expr        |
    method_call_expr   |
    member_access_expr |
    array_literal      |
    string_lit         |
    number_lit         |
    boolean_lit        |
    identifier
}
member_access_expr = { expr ~ "." ~ identifier }
call_expr          = { identifier ~ "(" ~ (expr ~ ("," ~ expr)*)? ~ ")" }
method_call_expr   = { expr ~ "." ~ identifier ~ "(" ~ (expr ~ ("," ~ expr)*)? ~ ")" }
binary_expr        = { expr ~ binary_op ~ expr }
binary_op          = { "==" | "!=" | "<=" | ">=" | "<" | ">" }
array_literal      = { "[" ~ (expr ~ ("," ~ expr)*)? ~ "]" }
condition          = { expr }

// Event Listeners (parsed into AST; NOT compiled or executed)
listener_decl = { "listener" ~ identifier ~ "(" ~ "event" ~ ":" ~ identifier ~ ")" ~ block }

// Tests and Mocks
test_decl = { "test" ~ string_lit ~ block }
mock_decl = { "mock" ~ identifier ~ "{" ~ (identifier ~ ":" ~ expr ~ ","?)+ ~ "}" }
```

---

## 2. Key Deviations from General Purpose Languages

- **No Class Methods:** Agents are data structures, not active classes. Execution happens via `execute AgentName.run(...)`.
- **`invoke:` and `using:` are mutually exclusive** on a tool — enforced by semantic analysis (Pass 2), not the grammar.
- **`client:` is only valid on `using: computer` tools** — enforced by semantic analysis (Pass 3, `E-CU02`).
- **`reason {}` calls LLM at runtime** — unlike all other workflow statements, `reason {}` is non-deterministic. Marked `W-CU01`-equivalent during semantic analysis.
- **Explicit Type Enforcement:** All workflow inputs, `let` bindings, and tool outputs map to declared types verified at compile time.

---

## 3. Capability Reference (`using_expr` terminals)

| Terminal | Spec | Mechanism | LLM at runtime? |
| --- | --- | --- | --- |
| `fetch` | 32 | `fetch()` HTTP calls | No |
| `playwright` | 32 | Headless browser | No |
| `bash` | 32 | `execFile` subprocess | No |
| `applescript` | 49 §1 | `osascript` via `execFile` | No |
| `computer` | 49 §2 | Screenshot + vision model + input sim | **Yes** |
| `mcp(Name)` | 26 | MCP tool server call | No |
| `baml(Name)` | 18 | BAML function call | Synthesis time only |
