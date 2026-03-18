# OpenClaw DSL: Type System & Static Analysis

Once the parser has created the `ast::Document`, the compiler enters Phase 2: Static Analysis.

The `.claw` DSL is strongly typed. The compiler must prove that the entire orchestration graph is mathematically sound *before* it generates any SDK code. 

## 1. The Three Passes of Validation

The analyzer reads the `ast::Document` and performs three sequential passes:

### Pass 1: Declaration Resolution (Symbol Table)
The compiler scans the AST and registers every `type`, `client`, `tool`, `agent`, and `workflow` into a global Symbol Table (`HashMap<String, SymbolInfo>`). 
* **Error Trigger:** If a user defines `agent Scraper` twice, fail with `CompilerError::DuplicateSymbol`.

### Pass 2: Reference Validation
The compiler walks through the agent, tool, and workflow definitions to ensure all referenced symbols exist.
* **Error Triggers:**
    * If `agent Scraper` says `tools = [WebScraper, FakeTool]`, but `tool FakeTool` was never defined. (`CompilerError::UndefinedTool`)
    * If an agent extends an unknown agent (`extends MissingAgent`).
    * If an agent uses an unknown `client`.

### Pass 3: Execution Type Checking (The Hard Part)
This is where the compiler proves the "Steel Tube" constraints. It walks the `workflow` AST nodes and validates data flow.

```claw
let data: SearchResult = execute Researcher.run(
    task: "Find data",
    require_type: SearchResult
)
```

**The Compiler guarantees:**
1. `Researcher` is a valid agent.
2. `SearchResult` is a valid `type` defined in the file.
3. The left-hand assignment (`data: SearchResult`) perfectly matches the right-hand constraint (`require_type: SearchResult`).
* **Error Trigger:** If the developer writes `let data: string = execute Researcher.run(require_type: SearchResult)`, the compiler fails instantly with `CompilerError::TypeMismatch`.

## 2. TypeBox Lowering

For Constrained Decoding to work, the high-level `.claw` types must be translated into raw JSON Schema (TypeBox format). As part of the Type System pass, the compiler generates a TypeBox representation for every `type` and `tool` in the AST.

**Input (`.claw` type):**
```claw
type ProductLink {
    name: string
    url: string
}
```

**Internal Compiler Output (TypeBox JSON Schema):**
```json
{
  "$id": "ProductLink",
  "type": "object",
  "properties": {
    "name": { "type": "string" },
    "url": { "type": "string" }
  },
  "required": ["name", "url"],
  "additionalProperties": false
}
```

The compiler attaches these lowered schemas directly to the AST nodes. They will be embedded into the final SDK as string literals, ready to be sent to the OpenClaw Gateway.

## 3. Boundary Safety Limitations

As documented, the internal `.claw` type system guarantees that data moving between *agents* and *workflows* is 100% typed.
However, for custom external tools like:
`invoke: module("scripts.scraper").function("run_scrape")`

The rust compiler cannot static-analyze the raw Python/TypeScript file. Therefore, `clawc` assumes the developer's `.claw` type signature is correct at compile-time, and generates Runtime assertion checks (Zod/Pydantic) at that boundary in the final SDK.
