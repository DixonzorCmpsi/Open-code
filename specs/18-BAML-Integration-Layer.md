# Phase 6D: BAML Integration Layer

OpenClaw's LLM boundary currently makes raw HTTP calls with no retry, no constrained decoding, and minimal prompt engineering. BAML solves these problems. This spec defines how `clawc` emits BAML definitions and how the gateway calls them.

**The developer writes ONLY `.claw`. BAML is an invisible compilation target.**

---

## 0. Goals & Non-Goals

### Goals (MUST do)
- Emit BAML files (`generated/baml_src/`) from `clawc build` — clients, types, and per-call-site functions
- Add two utility functions (`resolve_agents()` + `collect_call_sites()`) called during codegen — NOT a new pipeline stage. These run after semantic analysis passes and before code emission. They do NOT modify the pipeline's stage model or the `analyze_collecting()` function.
- Generate one BAML function per unique `(agent_name, return_type)` pair — NOT one per agent
- Tool-using agents (resolved.tools.len() > 0) SKIP BAML function generation entirely
- Gateway calls generated BAML client (typed, via dynamic import) with mtime-based hot reload
- Maintain fixed execution priority: Mock > Tools > BAML > Raw HTTP fallback
- BAML is OPTIONAL — gateway starts and all tests pass without `@boundaryml/baml` installed
- Pin BAML dependency to exact version (no caret range on 0.x)
- TypeBox validation after BAML is defense-in-depth (lenient: log warning on mismatch, don't reject valid BAML output)

### Non-Goals (MUST NOT do)
- Do NOT implement BAML-powered tool use loops (Phase 7 — agents with tools stay on gateway routing)
- Do NOT implement BAML streaming (Phase 7 — checkpoint system assumes complete responses)
- Do NOT implement conversation history / Memory.truncate() API (Phase 8)
- Do NOT implement a generic `BamlClient.call(name, args)` dynamic dispatch — use BAML's real generated typed client
- Do NOT use `minijinja` for BAML template emission (BAML's `{{ }}` syntax conflicts — use string building)
- Do NOT make BAML a required dependency — it is optional with graceful fallback
- Do NOT change the `.claw` language syntax to accommodate BAML — the developer sees only `.claw`
- Do NOT implement BAML function generation for agents with tools — even if some call sites are pure LLM
- Do NOT implement custom provider routing in the raw HTTP fallback — that's BAML's job
- Do NOT implement a BAML version negotiation protocol — if version mismatches, emit a clear error and fall back to raw HTTP

---

## 1. Architecture

### 1.1 Compilation Pipeline

```
example.claw
    │
    clawc build
    │
    ├── generated/claw/index.ts        (orchestration SDK — unchanged)
    ├── generated/claw/document.json    (compiled AST — unchanged)
    └── generated/baml_src/            (NEW — BAML project)
        ├── generators.baml            (BAML generator config)
        ├── clients.baml               (LLM provider configs)
        ├── types.baml                 (type/class definitions)
        └── functions.baml             (per-call-site BAML functions)
```

After `clawc build`, the developer (or CI) runs `npx baml-cli generate` to produce the typed BAML client from the `.baml` files. This is a TWO-STEP build:

```bash
openclaw build           # .claw → SDK + .baml files
npx baml-cli generate    # .baml files → baml_client/ (typed TS/Python)
```

`openclaw build` can orchestrate both steps automatically when BAML is installed.

### 1.2 Runtime Flow

```
Gateway: executeAgentRun()
    │
    ├── Mock check (spec 17 mock registry — HIGHEST priority)
    │
    ├── Tool check (agent.tools.length > 0 → gateway tool routing)
    │
    ├── BAML check (baml_client available → call typed BAML function)
    │
    └── Fallback (raw HTTP bridge in llm.ts — always available)
```

**Execution priority is explicit and fixed:** Mocks > Tools > BAML > Raw HTTP.

---

## 2. Resolving the Three FATAL Flaws

### 2.1 FATAL Fix: One Function Per Call Site, Not Per Agent

The original spec proposed "one BAML function per agent." This fails because:
- The same agent can be called with different kwargs across call sites
- The same agent can have different `require_type` across call sites

**Solution: Generate one BAML function per unique `(agent_name, require_type)` pair.**

```
execute Researcher.run(task: "query", require_type: SearchResult)
→ generates: ResearcherRun_SearchResult(task: string) -> SearchResult

execute Researcher.run(task: "query", context: data, require_type: VerifiedUser)
→ generates: ResearcherRun_VerifiedUser(task: string, context: string) -> VerifiedUser
```

**Naming convention:** `{AgentName}Run_{ReturnTypeName}`

If an agent call has NO `require_type`, the function returns `string`:
```
execute Researcher.run(task: "summarize")
→ generates: ResearcherRun_String(task: string) -> string
```

### 2.2 FATAL Fix: Use BAML's Real API (Generated Client, Not Dynamic Dispatch)

BAML does NOT have a `BamlClient.call(functionName, kwargs)` API. BAML generates typed client code that you import directly.

**The real integration pattern:**

Step 1 — `clawc build` emits `.baml` files to `generated/baml_src/`.

Step 2 — `baml-cli generate` produces `generated/baml_client/` with typed TypeScript:
```typescript
// generated/baml_client/index.ts (auto-generated by BAML CLI)
export async function ResearcherRun_SearchResult(args: {
  task: string;
}): Promise<SearchResult> { ... }

export async function ResearcherRun_VerifiedUser(args: {
  task: string;
  context?: string;
}): Promise<VerifiedUser> { ... }
```

Step 3 — The gateway imports the generated BAML client:
```typescript
// openclaw-gateway/src/baml-bridge.ts
let bamlFunctions: Record<string, (args: Record<string, unknown>) => Promise<unknown>> | null = null;

export async function loadBamlClient(workspaceRoot: string): Promise<void> {
  try {
    const clientPath = join(workspaceRoot, "generated", "baml_client", "index.js");
    const mod = await import(pathToFileURL(clientPath).href);
    bamlFunctions = {};
    for (const [name, fn] of Object.entries(mod)) {
      if (typeof fn === "function") {
        bamlFunctions[name] = fn as (args: Record<string, unknown>) => Promise<unknown>;
      }
    }
  } catch {
    bamlFunctions = null; // BAML not available
  }
}

export function callBamlFunction(
  functionName: string,
  args: Record<string, unknown>
): Promise<unknown> | null {
  if (!bamlFunctions || !bamlFunctions[functionName]) return null;
  return bamlFunctions[functionName](args);
}
```

### 2.3 FATAL Fix: Polymorphic Agents Generate Multiple Functions

The compiler resolves this during a NEW IR phase (see Section 3).

---

## 3. New Compiler Phase: Agent Resolution IR

### 3.1 Why This Is Needed

The BAML emitter needs information the raw AST does not provide:
- Flattened agent declarations (resolved `extends` chains)
- Aggregated call site signatures (all kwarg names + types per agent+return_type pair)
- Optional parameter detection (kwargs that appear in some call sites but not all)

This is semantic analysis work, NOT codegen work. Per spec 02, the compiler has 4 stages. We add a Phase 2.5:

```
1. Parsing → AST
2. Semantic Analysis → validated AST
2.5. Agent Resolution IR → ResolvedAgent + CallSiteMap (NEW)
3. TypeBox Lowering → schemas
4. Code Generation → SDK + BAML files
```

### 3.2 Data Structures

```rust
/// A fully resolved agent with inherited properties materialized
pub struct ResolvedAgent {
    pub name: String,
    pub client: Option<String>,          // Resolved from parent chain
    pub system_prompt: Option<String>,   // Resolved from parent chain
    pub tools: Vec<String>,             // Merged from parent chain (including +=)
    pub settings: AgentSettings,         // Merged from parent chain
    pub span: Span,
}

/// A unique call site signature for BAML function generation
pub struct CallSiteSignature {
    pub agent_name: String,
    pub return_type_name: String,        // e.g., "SearchResult" or "String"
    pub params: Vec<CallSiteParam>,      // Union of all kwargs for this (agent, return_type)
    pub baml_function_name: String,      // e.g., "ResearcherRun_SearchResult"
}

pub struct CallSiteParam {
    pub name: String,
    pub is_optional: bool,               // true if not present in all call sites
}
```

### 3.3 Resolution Algorithm

```rust
pub fn resolve_agents(document: &Document) -> Vec<ResolvedAgent> {
    document.agents.iter().map(|agent| {
        let mut resolved = ResolvedAgent {
            name: agent.name.clone(),
            client: agent.client.clone(),
            system_prompt: agent.system_prompt.clone(),
            tools: agent.tools.clone(),
            settings: agent.settings.clone(),
            span: agent.span.clone(),
        };

        // Walk the extends chain with cycle detection
        let mut parent_name = agent.extends.clone();
        let mut visited_parents = HashSet::new();
        visited_parents.insert(agent.name.clone());
        while let Some(ref name) = parent_name {
            if visited_parents.contains(name) {
                return Err(CompilerError::CircularAgentExtends {
                    agent_name: agent.name.clone(),
                    span: agent.span.clone(),
                });
            }
            visited_parents.insert(name.clone());
            // NOTE: resolve_agents() MUST run AFTER semantic Pass 2 which validates
            // that all extends targets exist. If not found, it's an internal error.
            if let Some(parent) = document.agents.iter().find(|a| &a.name == name) {
                if resolved.client.is_none() { resolved.client = parent.client.clone(); }
                if resolved.system_prompt.is_none() { resolved.system_prompt = parent.system_prompt.clone(); }
                if agent.tools.is_empty() {
                    resolved.tools = parent.tools.clone();
                }
                parent_name = parent.extends.clone();
            } else {
                // SAFETY: semantic Pass 2 validated this reference exists.
                // If we reach here, it's an internal compiler error.
                break;
            }
        }

        resolved
    }).collect()
}

pub fn collect_call_sites(document: &Document) -> Vec<CallSiteSignature> {
    let mut sites: HashMap<(String, String), Vec<Vec<String>>> = HashMap::new();

    // Walk all workflows and collect execute statements
    for workflow in &document.workflows {
        visit_block_for_call_sites(&workflow.body, &mut sites);
    }

    sites.into_iter().map(|((agent_name, return_type), kwarg_sets)| {
        let all_names: HashSet<String> = kwarg_sets.iter().flatten().cloned().collect();
        let params = all_names.into_iter().map(|name| {
            let present_in_all = kwarg_sets.iter().all(|set| set.contains(&name));
            CallSiteParam { name, is_optional: !present_in_all }
        }).collect();

        CallSiteSignature {
            baml_function_name: format!("{}Run_{}", agent_name, return_type),
            agent_name,
            return_type_name: return_type,
            params,
        }
    }).collect()
}
```

### 3.4 TDD Tests

1. **`test_resolve_agent_flattens_extends`** — SeniorResearcher extends Researcher → inherits client, system_prompt.
2. **`test_collect_call_sites_merges_kwargs`** — Same agent called with `(task)` and `(task, context)` → params: `[task: required, context: optional]`.
3. **`test_collect_call_sites_splits_by_return_type`** — Researcher called with `require_type: SearchResult` and `require_type: VerifiedUser` → two separate CallSiteSignatures.
4. **`test_resolve_agent_tool_inheritance`** — Child with `tools += [Extra]` → tools = parent.tools + [Extra].

---

## 4. BAML Code Generation

### 4.1 Emitter (`src/codegen/baml.rs`)

Uses direct string building (same pattern as `typescript.rs` and `python.rs`), NOT minijinja. BAML template syntax (`{{ }}`, `{% %}`) would conflict with minijinja's delimiters.

```rust
pub fn generate_baml(
    document: &Document,
    resolved_agents: &[ResolvedAgent],
    call_sites: &[CallSiteSignature],
) -> Result<BamlOutput, CompilerError> {
    Ok(BamlOutput {
        generators: emit_generators(),
        clients: emit_clients(document)?,
        types: emit_types(document)?,
        functions: emit_functions(resolved_agents, call_sites)?,
    })
}

pub struct BamlOutput {
    pub generators: String,
    pub clients: String,
    pub types: String,
    pub functions: String,
}
```

### 4.2 Generator Block (required by `baml-cli generate`)

```baml
// generators.baml
generator target {
  output_type "typescript"
  output_dir "../baml_client"
  version "0.70.0"
}
```

### 4.3 Function Emission

For each `CallSiteSignature`:

```rust
fn emit_function(
    sig: &CallSiteSignature,
    agent: &ResolvedAgent,
) -> String {
    let params = sig.params.iter()
        .map(|p| if p.is_optional {
            format!("  {}: string?", p.name)
        } else {
            format!("  {}: string", p.name)
        })
        .collect::<Vec<_>>()
        .join(",\n");

    let optional_guards = sig.params.iter()
        .filter(|p| p.is_optional)
        .map(|p| format!(
            "    {{% if {name} %}}\n    {name}: {{{{ {name} }}}}\n    {{% endif %}}",
            name = p.name
        ))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"function {fn_name}(
{params}
) -> {return_type} {{
  client {client}
  prompt #"
    {{{{ _.role("system") }}}}
    {system_prompt}

    {{{{ _.role("user") }}}}
    Task: {{{{ task }}}}
{optional_guards}
  "#
}}"#,
        fn_name = sig.baml_function_name,
        params = params,
        return_type = sig.return_type_name,
        client = agent.client.as_deref().unwrap_or("DefaultClient"),
        system_prompt = agent.system_prompt.as_deref().unwrap_or("You are a helpful assistant."),
    )
}
```

### 4.4 Type Mapping (`.claw` → BAML)

The `emit_types(document)?` function converts `.claw` type declarations to BAML `class` definitions. The mapping rules are:

| `.claw` Type | BAML Type | Notes |
|---|---|---|
| `string` | `string` | |
| `int` | `int` | |
| `float` | `float` | |
| `bool` | `bool` | |
| `list<T>` | `T[]` | Recursive: `list<list<string>>` → `string[][]` |
| `optional<T>` / `T?` | `T?` | BAML nullable syntax |
| User-defined `type Foo` | `class Foo` | Fields mapped recursively |

**Constraint mapping:** `.claw` field constraints (e.g., `@min(1)`, `@max(100)`) are emitted as BAML `@check` annotations:

```baml
// .claw: type SearchResult { confidence_score: float @min(0) @max(1) }
class SearchResult {
  confidence_score float @check(min_val, {{ this >= 0 and this <= 1 }})
}
```

**Nested types:** If `type A` contains a field of `type B`, both `class A` and `class B` are emitted. The emitter topologically sorts type declarations to avoid forward references.

**Unsupported:** Recursive types are rejected at compile time (Spec 05 §1 circular type detection), so the emitter does not need to handle them.

### 4.5 Tool-Using Agents: Skipped

Agents where `resolved.tools.len() > 0` do NOT generate BAML functions. The gateway handles tool routing for these agents using the existing `executeAgentRun()` code path. BAML-powered tool use is a Phase 7 feature.

---

## 5. Gateway Integration

### 5.1 Execution Order (Definitive)

```typescript
async function executeAgentRun(
  compiled, state, executeRun, statementPath, workspaceRoot, checkpoints,
  mockRegistry?: MockRegistry
): Promise<unknown> {
  const agent = findAgent(compiled.document, executeRun.agent_name);
  const returnSchema = /* ... existing schema building ... */;

  // Priority 1: Mock interception (test mode)
  if (mockRegistry) {
    const mock = mockRegistry.lookup(executeRun.agent_name);
    if (mock !== null) return validateToolResult(mock, returnSchema);
  }

  // Priority 2: Tool routing (Browser, custom tools)
  if (/* existing tool routing conditions */) {
    return /* existing tool routing */;
  }

  // Priority 3: BAML (if available and agent has no tools)
  if (agent.tools.length === 0) {
    const bamlResult = await tryBamlCall(agent, executeRun, kwargs, workspaceRoot);
    if (bamlResult !== null) {
      return validateToolResult(bamlResult, returnSchema);
    }
  }

  // Priority 4: Raw HTTP fallback
  return validateToolResult(
    await generateStructuredResult({ /* existing */ }),
    returnSchema
  );
}
```

### 5.2 BAML Call Helper

```typescript
async function tryBamlCall(
  agent: AgentDecl,
  executeRun: StatementExecuteRun,
  kwargs: Record<string, unknown>,
  workspaceRoot: string
): Promise<unknown | null> {
  const returnTypeName = extractCustomTypeName(executeRun.require_type) ?? "String";
  const functionName = `${agent.name}Run_${returnTypeName}`;

  const result = callBamlFunction(functionName, kwargs);
  if (result === null) return null; // BAML not available

  return result;
}
```

### 5.3 Hot Reload Support

```typescript
// baml-bridge.ts
let bamlFunctions: Record<string, Function> | null = null;
let bamlLoadedAt = 0;

export async function loadBamlClient(workspaceRoot: string, force = false): Promise<void> {
  const clientPath = join(workspaceRoot, "generated", "baml_client", "index.js");
  try {
    const stat = await import("node:fs/promises").then(fs => fs.stat(clientPath));
    const mtime = stat.mtimeMs;
    if (!force && bamlFunctions && mtime <= bamlLoadedAt) return;

    // Dynamic import with cache-busting query string
    const mod = await import(`${pathToFileURL(clientPath).href}?t=${mtime}`);
    bamlFunctions = {};
    for (const [name, fn] of Object.entries(mod)) {
      if (typeof fn === "function") bamlFunctions[name] = fn;
    }
    bamlLoadedAt = mtime;
  } catch {
    bamlFunctions = null;
  }
}
```

The gateway calls `loadBamlClient(workspaceRoot)` on every request. The function checks the file's mtime and only reloads if the generated client has changed. This supports hot reload during `openclaw dev`.

---

## 6. Validation Strategy: BAML Types Win, TypeBox Is Defense-in-Depth

BAML validates output using its own type system. TypeBox validates using OpenClaw's schema.

**Rule:** If BAML returns a result, the TypeBox validation still runs. If TypeBox validation fails after BAML succeeds, this indicates a codegen bug (BAML types drifted from OpenClaw types). Per the fail-fast philosophy, the gateway MUST throw a `SchemaDegradationError` with a descriptive message including both the BAML function name and the specific TypeBox validation failure. This is NOT silently swallowed.

**In practice:** TypeBox validation should almost never fail after BAML succeeds. If it does, it's a bug in the BAML emitter's type mapping and must be surfaced immediately, not hidden behind a warning.

---

## 7. Dependencies

### 7.1 Gateway

```json
{
  "optionalDependencies": {
    "@boundaryml/baml": "0.70.0"
  }
}
```

**Pinned exact version** (not caret range). BAML is pre-1.0 and breaking changes occur between minors.

### 7.2 BAML CLI

The developer installs `@boundaryml/baml` globally or as a project dependency. `openclaw build` detects if `baml-cli` is available and runs `baml-cli generate` automatically after emitting `.baml` files.

---

## 8. Behavioral Parity: What Happens Without BAML

| Feature | With BAML | Without BAML |
|---------|-----------|-------------|
| `retries = 3` | BAML retries | Fallback: raw HTTP, retry count logged as warning |
| `temperature: 0.1` | Passed to provider | Logged as warning, not applied |
| `provider = "custom"` | Routes via BAML | Falls to mock with warning |
| `@regex(...)` constraint | Token-level (if supported) | Post-parse validation |

**Important:** When BAML is not available, the compiler emits a warning:
```
warning: BAML runtime not found. LLM features (retries, temperature, custom providers)
         will be degraded. Install @boundaryml/baml for full functionality.
```

This ensures developers know the system is running in fallback mode.

---

## 9. Error Types

### 9.1 Compile-Time Errors

Add to `src/errors.rs`:

```rust
CompilerError::BamlSignatureConflict {
    agent_name: String,
    message: String,
    span: Span,
}

CompilerError::CircularAgentExtends {
    agent_name: String,
    span: Span,
}
```

### 9.2 Runtime Error Semantics Across Execution Chain

The fixed execution priority (Mock > Tools > BAML > Raw HTTP) produces these error types:

| Source | Error thrown | Catchable in try/catch as |
|--------|------------|--------------------------|
| Mock returns data that fails TypeBox | `SchemaDegradationError` | `SchemaDegradationError` |
| BAML call fails (network/provider error) | `AgentExecutionError` | `AgentExecutionError` |
| BAML retry exhaustion (all retries failed) | `AgentExecutionError` | `AgentExecutionError` |
| BAML returns data that fails TypeBox | `SchemaDegradationError` | `SchemaDegradationError` |
| Raw HTTP call fails | `AgentExecutionError` | `AgentExecutionError` |
| Tool execution fails | `ToolExecutionError` | `ToolExecutionError` |
| Tool sandbox timeout | `ToolExecutionError` | `ToolExecutionError` |

All three built-in error types (`AgentExecutionError`, `SchemaDegradationError`, `ToolExecutionError`) are registered in the symbol table (Spec 15) and can be caught by any `catch (e: ErrorType)` clause.

---

## 10. TDD Tests

### Compiler Tests

1. **`test_resolve_agent_inherits_client`**
2. **`test_resolve_agent_inherits_system_prompt`**
3. **`test_collect_call_sites_single_agent_single_type`**
4. **`test_collect_call_sites_single_agent_two_types`** → Two functions generated
5. **`test_collect_call_sites_optional_params`** → `context` optional when not in all sites
6. **`test_emit_baml_client_openai`** → Snapshot test
7. **`test_emit_baml_type_with_constraints`** → Snapshot test
8. **`test_emit_baml_function_with_optional_param`** → Snapshot test
9. **`test_emit_baml_generator_block`** → Contains output_type and version
10. **`test_skip_tool_using_agent`** → Agent with tools → no BAML function

### Gateway Tests

11. **`test_baml_bridge_returns_null_when_not_installed`**
12. **`test_baml_bridge_calls_correct_function_name`**
13. **`test_execution_order_mock_over_baml`** — Mock registry takes priority
14. **`test_execution_order_tools_over_baml`** — Tool-using agent skips BAML
15. **`test_hot_reload_picks_up_new_baml_client`** — Change mtime, verify reload

---

## 11. Limitations & Future Work

- **Tool-using agents skip BAML** (Phase 7: BAML-powered tool use loops)
- **No streaming** (Phase 7: BAML streaming + gateway checkpoint integration)
- **No conversation history** (`Memory.truncate()` + session API → Phase 8)
- **Per-call-site function generation** produces many functions for polymorphic agents. If this becomes unwieldy, Phase 7 can introduce BAML dynamic dispatch.
