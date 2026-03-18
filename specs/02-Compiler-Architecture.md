# OpenClaw Compiler (`clawc`) Architecture

The OpenClaw Compiler (`clawc`) is a standalone Rust binary responsible for parsing `.claw` language files, validating types, enforcing agent boundaries, and generating language-specific SDKs (TypeScript and Python).

## 1. High-Level Pipeline

The compiler follows a four-stage pipeline:

1. **Lexing & Parsing (AST Generation)**
2. **Semantic Analysis (Type Checking)**
3. **Internal Representation (TypeBox Lowering)**
4. **Code Generation (SDK Emission)**

---

## 2. Architectural Guardrails (Goals & Non-Goals)

To ensure this tooling remains focused on deterministic execution, we must strictly define what the compiler **is** and **is not** responsible for.

### 🎯 Goals (What we MUST do)
* **Agent Routing Safety over Runtime Errors:** If an agent attempts to call a tool it doesn't have access to, or pass an output shape into an agent input that doesn't match, `clawc` must catch this *before* generating the SDK.
* *(Note on Limitation)*: The compiler cannot perfectly guarantee the type safety of the *underlying Python/TS execution function* itself. Therefore, the boundary between the generated SDK and the raw script must be validated at Runtime (e.g., using Pydantic/Zod). 
* **100% Deterministic State Boundaries:** The space between agent runs (e.g., extracting data, validating it, looping it) must be fully controlled by the generated code. The LLM is only responsible for the generative text within the execution block.
* **Maximum Type Strictness:** Every `.claw` type must lower into an airtight TypeBox schema. We must use Constrained Decoding best practices (no raw `anyOf` or loose `object` definitions where possible).
* **Zero-Dependency Core:** The `clawc` binary itself must compile to a standalone executable that relies on nothing else on the host machine.

### 🚫 Non-Goals (What we MUST NOT do)
* **No "Auto-Prompting" or Magic:** The compiler translates explicitly what the developer writes. It should not try to automatically "improve" or re-write the user's `system_prompt` values.
* **No Runtime Engine within `clawc`:** The compiler does NOT connect to LLMs. It generates TypeScript/Python SDKs that do. We are not building an inference engine; we are building an orchestration generator.
* **No Unbounded Agent Loops by Default:** By default, workflows should not allow an agent to infinitely call itself. Iteration must be definitively programmed using `for` loops or explicit `max_steps` configurations.

---

## 2. Stage 1: Lexing & Parsing (AST Generation)

**Tooling:** Following Rust compilation best practices, we will use parser combinator libraries like `winnow` (for performance and fine-grained control) or `pest` (for PEG declarative grammar files) to define the `.claw` syntax.

**Process & Best Practices:**
1. The compiler reads `main.claw`.
2. The Lexer breaks the raw text into tokens. **Crucially, the lexer must track the exact line and character "span"** of every token. This ensures that if the user makes a syntax error, `clawc` can point exactly to the broken line, mirroring the high-quality error reporting of the Rust compiler itself.
3. The Parser groups these tokens into a strongly typed Abstract Syntax Tree (AST). 
4. **Macro Utilization:** We will leverage Rust's procedural macros to reduce AST boilerplate (e.g., utilizing `astmaker`-style patterns) defining AST data nodes exhaustively.

**AST Example:**
```rust
struct AgentDecl {
    name: String,
    model: ModelEnum,
    tools: Vec<String>,
    settings: SettingsBlock,
    // Maintaining source-file traceability for semantic error reporting
    span: Span, 
}
```

---

## 2. Stage 2: Semantic Analysis

Before generating any code, the compiler must prove that the `.claw` code is logical and safe.

**Validation Rules:**
1. **Tool Existence:** If `agent Researcher` uses `tools = [WebScraper]`, the compiler checks if `WebScraper` is actually defined in the AST. If not, it throws a compile-time error.
2. **Type Compatibility:** If a workflow executes a tool that returns `ScrapedData`, and passes that directly into an agent prompt that expects `AnalyzedData`, the semantic analyzer throws a type-mismatch error.
3. **Graph Cyclic Checking:** Ensures that agent delegation loops cannot result in an infinite, unbounded cycle (unless explicitly marked as asynchronous).

---

## 3. Stage 3: Internal Representation (TypeBox Lowering)

This is the core "Magic" of the compiler.

The compiler takes high-level structs and lowers them into the specific JSON Schema / TypeBox representation that the OpenClaw Gateway requires for **Constrained Decoding**.

For example, a `.claw` type:
```claw
type SearchResult {
    url: string
    tags: list<string>
}
```

Is translated internally in the compiler's memory to the exact syntax required by OpenClaw's Gateway tools.

---

## 4. Stage 4: Code Generation (Emitters)

The final step is emitting the code the developer will actually use. The compiler uses a templating engine (like `askama` or `tera` in Rust) to write the SDK files.

**Target 1: TypeScript (`clawc generate --target ts`)**
Generates `generated/claw.ts`.
This file will contain TypeScript interfaces mapping to the `.claw` types, and asynchronous wrapper functions that handle the WebSocket connection to `openclaw-gateway`.

**Target 2: Python (`clawc generate --target python`)**
Generates `generated/claw.py`.
Creates Pydantic models for the `.claw` types and asynchronous Python functions (using `asyncio` and `websockets`) to trigger the OpenClaw Gateway.

## 5. Development Milestones

*   **Milestone 1:** Build the `pest` grammar file (`claw.pest`) and parse a basic `agent` block into Rust structs.
*   **Milestone 2:** Implement the Semantic Analyzer to catch missing tools/types.
*   **Milestone 3:** Build the TypeBox IR layer to convert `type` blocks into JSON Schemas.
*   **Milestone 4:** Build the TypeScript emitter.
