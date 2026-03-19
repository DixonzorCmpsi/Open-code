# Phase 6A: Compiler Completeness

Remaining compiler features defined in specs 02-05 but not yet implemented. Each feature includes TDD tests, algorithms, and semantic rules.

**Prerequisite specs:** `03-Grammar.md`, `04-AST-Structures.md`, `05-Type-System.md`, `08-Testing-Spec.md`.

**IMPORTANT: Spec Updates Required.** This spec requires changes to `specs/03-Grammar.md` and `specs/04-AST-Structures.md`. These updates MUST be applied BEFORE implementing code.

---

## 0. Goals & Non-Goals

### Goals (MUST do)
- Parse `try/catch` with REQUIRED explicit catch type (no untyped catch bindings)
- Parse `continue` and `break` with compile-time loop-depth validation
- Parse all 6 binary operators (`==`, `!=`, `<`, `>`, `<=`, `>=`) with type-safe semantic checks
- Detect circular type references in Pass 1 (implements Spec 05 §1; includes through `list<list<Custom>>`)
- Verify exhaustive return paths in Pass 3 (all workflow code paths reach `return`)
- Parse `member_access_expr` for `result.field` syntax (single-level and chained: `a.b.c`)
- Parse `assert` statement (grammar + AST only — runtime semantics owned by Spec 17)
- Collect up to 50 errors per compilation pass instead of halting on first error
- Register built-in error types (`AgentExecutionError`, `SchemaDegradationError`, `ToolExecutionError`) in the symbol table so catch clauses can reference them
- Parse `else if` chaining — `else if (cond) { }` desugars into nested `IfCond` via the `ElseBranch::ElseIf` variant (see Spec 04)
- Parse expression iterators in `for` loops — `for (item in result.tags)` is valid; the iterator is any `SpannedExpr`
- Wrap all `Expr` references in `SpannedExpr { expr: Expr, span: Span }` per Spec 04 §1 ("every single node must retain its Span")

### Non-Goals (MUST NOT do)
- Do NOT implement dead code warnings (Phase 7 — warn on statements after `return`)
- Do NOT implement `async`/`await` keywords (not in the `.claw` language)
- Do NOT add new data types (no `Map`, `Optional`, `Union` — only what spec 04 defines)
- Do NOT implement operator precedence or expression grouping with parentheses beyond what the current grammar supports
- Do NOT refactor the parser to use error recovery (winnow fails on first parse error; multi-error collection is SEMANTIC only)
- Do NOT add new circular detection for agent DELEGATION loops (spec 02 mandates this but it requires call-graph analysis across workflows, which is Phase 7). However, circular EXTENDS chains (A extends B, B extends A) MUST be detected — this is covered by Spec 18's `resolve_agents()` which includes an extends-chain cycle guard.
- Do NOT change the public `analyze()` function signature — add `analyze_collecting()` alongside it

- Do NOT perform expression-iterator item-type inference beyond a simple identifier lookup in Phase 6. `for (item in result.tags)` parses and executes, but compile-time item binding only occurs when the iterator resolves to a known list-typed variable.


---

## 1. try/catch Statement

### Spec 03 Grammar Update (MANDATORY)

Replace the existing `try_stmt` rule with:
```peg
try_stmt = { "try" ~ block ~ "catch" ~ "(" ~ identifier ~ ":" ~ data_type ~ ")" ~ block }
```
The catch type is **REQUIRED** (not optional). This resolves the three-way contradiction between spec 03 (optional), spec 04 (Option), and this spec (required).

**Rationale:** OpenClaw has no `any` or `dynamic` type. An untyped catch binding would create an untypeable variable. Requiring explicit types keeps the type system sound.

### Spec 04 AST Update (MANDATORY)

```rust
Statement::TryCatch {
    try_body: Block,
    catch_name: String,
    catch_type: DataType,  // REQUIRED — not Option<DataType>
    catch_body: Block,
    span: Span,
}
```

### Built-in Error Types

The following error types are implicitly available in every `.claw` file (no import needed):
- `AgentExecutionError` — thrown when an agent execution fails
- `SchemaDegradationError` — thrown when LLM output is all zero-values
- `ToolExecutionError` — thrown when a custom tool exits with non-zero code

These are registered in the symbol table during Pass 1 as built-in types. They do NOT need to be declared by the user.

### Gateway Traversal Update (MANDATORY)

**IMPORTANT:** Do NOT use JavaScript `try/catch` to implement AST-level try/catch. The traversal engine is frame-based and async — a JS `try` block exits immediately after pushing a frame, but the frame executes in a future loop iteration. JS `catch` will never see errors from that execution.

**Correct approach:** Use a new frame kind `"try_catch"` that stores the catch handler. When ANY error propagates up through the frame stack and encounters a `try_catch` frame, the error is caught, bound to the catch variable, and the catch body is pushed as a new frame.

Add to `executeStatement` in `traversal.ts`:
```typescript
case "TryCatch": {
  const { try_body, catch_name, catch_type, catch_body } = payload as TryCatchPayload;
  frame.nextIndex += 1;

  // Push a try_catch sentinel frame that stores the catch handler
  state.frames.push({
    kind: "try_catch",
    statementPath,
    catchName: catch_name,
    catchBodyPath: `${statementPath}/catch_body`,
  });

  // Push the try body as a regular block frame on top
  state.scopes.push({});
  state.frames.push({
    kind: "block",
    blockPath: `${statementPath}/try_body`,
    nextIndex: 0,
    createdScope: true,
  });

  await checkpoints.checkpoint(state, statementPath, "try_catch_enter");
  return;
}
```

**Error handling in the main traversal loop:** Wrap the existing `while (state.frames.length > 0)` loop body in a try/catch. When an error occurs:
```typescript
try {
  // ... existing frame execution ...
} catch (error) {
  // Walk up the frame stack looking for a try_catch frame
  let caught = false;
  while (state.frames.length > 0) {
    const top = state.frames[state.frames.length - 1]!;
    state.frames.pop();
    if (top.createdScope) state.scopes.pop();
    if (top.kind === "try_catch") {
      // Found the catch handler — bind error and push catch body
      const errorValue = error instanceof Error ? error.message : String(error);
      state.scopes.push({ [top.catchName]: errorValue });
      state.frames.push({
        kind: "block",
        blockPath: top.catchBodyPath,
        nextIndex: 0,
        createdScope: true,
      });
      caught = true;
      await checkpoints.checkpoint(state, top.statementPath, "try_catch_caught");
      break;
    }
  }
  if (!caught) throw error; // No try_catch frame found — propagate
}
```

Add `"try_catch"` to the `ExecutionFrame` union type in `types.ts`:
```typescript
export interface TryCatchFrame {
  kind: "try_catch";
  statementPath: string;
  catchName: string;
  catchBodyPath: string;
}

export type ExecutionFrame = BlockFrame | LoopFrame | TryCatchFrame;
```

### TDD Tests

1. **`test_parse_try_catch`** — Parses try/catch with explicit type. Assert `TryCatch` node with correct fields.
2. **`test_parse_nested_try_catch`** — `try { try { A } catch (e1: Error) { B } } catch (e2: Error) { C }` — each catch binds to innermost try.
3. **`test_reject_catch_without_type`** — `catch (e) { ... }` → `CompilerError::ParseError`.
4. **`test_reject_try_without_catch`** — `try { ... }` alone → `CompilerError::ParseError`.
5. **`test_semantic_catch_type_must_exist`** — `catch (e: NonExistentType)` → `CompilerError::UndefinedType`. (Built-in types like `AgentExecutionError` pass.)

### Semantic Rules

- `catch_name` is scoped to `catch_body` only.
- `catch_type` MUST exist in symbol table (built-in or user-defined).
- Before validating `catch_body`, clone the current `TypeEnv` and insert `{ catch_name: TypeShape::Custom(catch_type_name) }` (or the equivalent built-in error type shape) into that catch-local env only.
- Both `try_body` and `catch_body` contribute to return analysis independently.

---

## 2. `continue` and `break` Statements

### Spec 04 AST (already defined, needs implementation)

```rust
Statement::Continue(Span)
Statement::Break(Span)
```

### Gateway Traversal Update (MANDATORY)

```typescript
case "Continue": {
  // Pop frames until we find a loop frame, then advance the loop
  while (state.frames.length > 0) {
    const top = state.frames[state.frames.length - 1];
    state.frames.pop();
    if (top.createdScope) state.scopes.pop();
    if (top.kind === "loop") { break; }
  }
  await checkpoints.checkpoint(state, statementPath, "continue");
  return;
}
case "Break": {
  // Pop frames until we find AND remove a loop frame
  while (state.frames.length > 0) {
    const top = state.frames[state.frames.length - 1];
    state.frames.pop();
    if (top.createdScope) state.scopes.pop();
    if (top.kind === "loop") { break; }
  }
  await checkpoints.checkpoint(state, statementPath, "break");
  return;
}
```

### Error Type Addition

```rust
CompilerError::InvalidControlFlow {
    keyword: String,  // "continue" or "break"
    span: Span,
}
```

**Add to `errors.rs` `span()` method** for the new variant.

### TDD Tests

1. **`test_parse_continue`** � Parse continue in for loop body.
2. **`test_parse_break`** � Parse break in for loop body.
3. **`test_semantic_reject_continue_outside_loop`** ? `InvalidControlFlow`.
4. **`test_semantic_reject_break_outside_loop`** ? `InvalidControlFlow`.
5. **`test_semantic_accept_continue_in_nested_if_inside_loop`** ? `Ok(())`.

### Loop Depth Tracking

Track loop nesting depth in semantic Pass 3. Increment on `ForLoop`, decrement on leaving. If depth is 0 when encountering `continue`/`break`, emit `InvalidControlFlow`.

---

## 3. Full Binary Operators

### Grammar (parse order matters)

```peg
binary_op = { "<=" | ">=" | "!=" | "==" | "<" | ">" }
```
Parse multi-character operators FIRST to prevent `<=` being consumed as `<` then `=`.

### AST (spec 04 update)

```rust
pub enum BinaryOp {
    Equal, NotEqual, LessThan, GreaterThan, LessEq, GreaterEq,
}
```

### Gateway Traversal Update (type-safe, NO `as number` cast)

```typescript
case "BinaryOp": {
  const { left, op, right } = payload as { left: Expr; op: string; right: Expr };
  const l = await evaluateExpr(..., left, ...);
  const r = await evaluateExpr(..., right, ...);
  let result: boolean;
  switch (op) {
    case "Equal":      result = JSON.stringify(l) === JSON.stringify(r); break;
    case "NotEqual":   result = JSON.stringify(l) !== JSON.stringify(r); break;
    case "LessThan":
    case "GreaterThan":
    case "LessEq":
    case "GreaterEq": {
      if (typeof l !== "number" || typeof r !== "number") {
        throw new Error(`Comparison operator ${op} requires numeric operands, got ${typeof l} and ${typeof r}`);
      }
      switch (op) {
        case "LessThan":    result = l < r; break;
        case "GreaterThan": result = l > r; break;
        case "LessEq":      result = l <= r; break;
        case "GreaterEq":   result = l >= r; break;
      }
      break;
    }
    default: throw new Error(`Unknown binary operator ${op}`);
  }
  await checkpoints.checkpoint(state, statementPath, "binary_op", { result });
  return result;
}
```

### TDD Tests

1-4: Parse each operator. 5: Semantic reject comparison on string. 6: Snapshot all 6 operators.

### Semantic Rules

- `==` and `!=` accept any pair of operands that resolve to the SAME type.
- `<`, `>`, `<=`, and `>=` require numeric operands (`int` or `float`). Mixed `int`/`float` comparisons are allowed; non-numeric operands MUST produce `CompilerError::TypeMismatch`.

---

## 4. Circular Type Detection

> **Spec 05 §1** mandates circular type detection during Pass 1. This section provides the Phase 6 implementation of that requirement.

### Algorithm (returns Vec for multi-error collection)

```rust
fn detect_circular_types(document: &Document) -> Vec<CompilerError> {
    let mut errors = Vec::new();
    for type_decl in &document.types {
        let mut path = Vec::new();
        if let Err(e) = check_type_cycle(document, &type_decl.name, &mut path, &type_decl.span) {
            errors.push(e);
        }
    }
    errors
}

fn check_type_cycle(
    document: &Document, type_name: &str,
    path: &mut Vec<String>, origin_span: &Span,
) -> CompilerResult<()> {
    if path.contains(&type_name.to_owned()) {
        return Err(CompilerError::CircularType {
            type_name: type_name.to_owned(),
            cycle_path: path.clone(),
            span: origin_span.clone(),
        });
    }
    path.push(type_name.to_owned());
    if let Some(type_decl) = document.types.iter().find(|t| t.name == type_name) {
        for field in &type_decl.fields {
            check_data_type_cycle(document, &field.data_type, path, origin_span)?;
        }
    }
    path.pop();
    Ok(())
}

fn check_data_type_cycle(
    document: &Document, data_type: &DataType,
    path: &mut Vec<String>, origin_span: &Span,
) -> CompilerResult<()> {
    match data_type {
        DataType::Custom(name, _) => check_type_cycle(document, name, path, origin_span),
        DataType::List(inner, _) => check_data_type_cycle(document, inner, path, origin_span),
        _ => Ok(()),
    }
}
```

**Key fix:** `check_data_type_cycle` recursively handles `List(List(Custom(...)))` nested lists — the GAN Round 1 flaw.

### Error Type

```rust
CompilerError::CircularType {
    type_name: String,
    cycle_path: Vec<String>,
    span: Span,
}
```

**MUST add to `span()` match arm in `errors.rs`.**

### TDD Tests

1. `test_rejects_direct_cycle` — A→B→A
2. `test_rejects_self_reference` — A→A
3. `test_rejects_indirect_cycle` — A→B→C→A
4. `test_accepts_diamond` — A→B, A→C, B→D, C→D (no cycle)
5. `test_rejects_cycle_through_list` — `type A { items: list<B> }` + `type B { parent: A }`
6. `test_rejects_cycle_through_nested_list` — `type A { items: list<list<B>> }` + `type B { a: A }`

---

## 5. Exhaustive Return Analysis

### Algorithm

```rust
fn check_exhaustive_returns(document: &Document) -> Vec<CompilerError> {
    let mut errors = Vec::new();
    for workflow in &document.workflows {
        if workflow.return_type.is_some() && !block_always_returns(&workflow.body) {
            errors.push(CompilerError::MissingReturn {
                workflow_name: workflow.name.clone(),
                span: workflow.span.clone(),
            });
        }
    }
    errors
}
```

**Void workflows:** If `return_type` is `None`, the check is SKIPPED. A void workflow with a `return value` statement is a type mismatch caught in Pass 3, not a missing-return error.

**Dead code after return:** NOT an error in Phase 6. Dead code detection is deferred to Phase 7 as a compiler warning. The algorithm uses `.iter().any()` which correctly identifies that a block returns even if the return is in the middle.

### Exception Semantics for TryCatch

Both branches must return because at runtime, exactly one will execute:
- If try succeeds → catch is skipped
- If try throws → catch executes

Therefore: `always_returns(try) AND always_returns(catch)`.

### break/continue Interaction

- `continue` does NOT satisfy return (jumps to next iteration)
- `break` does NOT satisfy return (exits loop)
- `ForLoop` does NOT guarantee return (may execute 0 times)

### TDD Tests

1-7 as originally specified, plus:
8. **`test_void_workflow_skips_return_check`** — `workflow F(x: string) { let a = x }` → `Ok(())` (no return_type).

---

## 6. Multi-Error Collection

### Top-Level API

```rust
pub fn analyze_collecting(document: &Document) -> CompilationReport {
    let mut errors = Vec::new();
    match SymbolTable::build(document) {
        Ok(symbols) => {
            errors.extend(detect_circular_types(document));
            errors.extend(validate_references_collecting(document, &symbols));
            if errors.len() < 50 {
                errors.extend(validate_types_collecting(document, &symbols));
            }
            if errors.len() < 50 {
                errors.extend(check_exhaustive_returns(document));
            }
        }
        Err(e) => errors.push(e),
    }
    errors.sort_by_key(|e| e.span().map_or(usize::MAX, |s| s.start));
    errors.truncate(50);
    CompilationReport { errors }
}
```

**Span-less errors sort LAST** (using `usize::MAX`), not first.

### `_collecting` Function Pattern

`validate_references_collecting` and `validate_types_collecting` are new functions that accumulate errors into a `Vec<CompilerError>` instead of returning `Err` on the first failure. They are parallel implementations of the existing `validate_references` and `validate_types`.

### LSP Update (MANDATORY)

The LSP (`src/lsp.rs` `diagnostics_for_source`) MUST call `analyze_collecting()` instead of `analyze()` to report all diagnostics, not just the first.

### `into_result()` Preservation

```rust
impl CompilationReport {
    pub fn into_result(self) -> CompilerResult<()> {
        if let Some(first) = self.errors.into_iter().next() {
            Err(first)
        } else {
            Ok(())
        }
    }
}
```

This is a backwards-compatibility shim ONLY for callers that haven't migrated. New code SHOULD use `analyze_collecting()` directly.

### `analyze()` Delegation (MANDATORY)

The existing `analyze()` function MUST delegate to `analyze_collecting().into_result()` so that all Phase 6 checks apply to `clawc build` and any legacy callers as well.

### New Error Variants — `span()` Update

**MUST** add match arms to the existing `span()` method in `errors.rs` for:
- `CircularType { span, .. } => Some(span)`
- `MissingReturn { span, .. } => Some(span)`
- `InvalidControlFlow { span, .. } => Some(span)`
- `InvalidAssertOutsideTest { span, .. } => Some(span)`

---

## 7. Spec 03 Grammar Additions (Summary)

The following rules MUST be added to `specs/03-Grammar.md`:

```peg
// Member access (field access without method call)
member_access_expr = { expr ~ "." ~ identifier }

// Assert statement (test blocks only)
assert_stmt = { "assert" ~ expr ~ ("," ~ string_lit)? }

// Updated statement list
statement = {
    let_stmt | for_stmt | if_stmt | try_stmt |
    execute_stmt | return_stmt | continue_stmt | break_stmt |
    assert_stmt | expr
}
```

`member_access_expr` enables `result.tags` (field access) which is required for `result.tags.length()` (method call on accessed field).






