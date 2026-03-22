// src/codegen/skill_spec.rs
// Generates generated/specs/tools/<ToolName>.md — the synthesis execution prompt.
//
// Each spec is a direct imperative instruction to the synthesis agent, NOT a
// description document. The agent receives the spec as its entire prompt and
// must: (1) write the TypeScript file, (2) output the SYNTHESIS_COMPLETE sentinel.
//
// Design principles:
//   - Scaffold-first: 80% of the code is pre-written, only the body is left open
//   - Inlined types: all type definitions are included, no "go read other files"
//   - Curated research: capability-specific patterns with actual code examples
//   - Constrained output: acceptance checklist + explicit sentinel at the end
//   - Zero ambiguity: model knows exactly what to write and where

use std::fs;
use std::path::Path;

use crate::ast::{DataType, Document, Expr, ExpectOp, ToolDecl, UsingExpr};
#[allow(unused_imports)]
use crate::ast::Constraint;
use crate::errors::{CompilerError, CompilerResult};

pub fn generate(document: &Document, project_root: &Path) -> CompilerResult<()> {
    let has_synthesis = document.tools.iter().any(|t| t.using.is_some());
    if !has_synthesis {
        return Ok(());
    }

    let specs_dir = project_root.join("generated").join("specs").join("tools");
    fs::create_dir_all(&specs_dir).map_err(|e| CompilerError::IoError {
        message: format!("failed to create specs directory: {e}"),
        span: 0..0,
    })?;

    for tool in &document.tools {
        if tool.using.is_none() {
            continue;
        }
        let content = emit_skill_spec(tool, document);
        let filename = format!("{}.md", tool.name);
        fs::write(specs_dir.join(&filename), &content).map_err(|e| CompilerError::IoError {
            message: format!("failed to write skill spec {}: {e}", filename),
            span: tool.span.clone(),
        })?;
    }

    Ok(())
}

fn emit_skill_spec(tool: &ToolDecl, document: &Document) -> String {
    let mut out = String::new();

    let tool_name = &tool.name;
    let output_path = format!("generated/tools/{tool_name}.ts");
    let args_sig = format_args_signature(tool);
    let return_ts = format_return_type_ts(tool.return_type.as_ref());
    let input_type_ts = format_input_type_ts(tool);

    // ── Header: imperative task statement ─────────────────────────────────────
    out.push_str(&format!("# TASK: Write `{output_path}`\n\n"));
    out.push_str(&format!(
        "You are a TypeScript expert. Write a complete, working implementation of `{tool_name}` \
         to the file `{output_path}`.\n\n"
    ));
    out.push_str("**Do not explain. Do not ask questions. Write the file, then output:**\n");
    out.push_str(&format!("`SYNTHESIS_COMPLETE: {tool_name}`\n\n"));
    out.push_str("---\n\n");

    // ── Exact output path ─────────────────────────────────────────────────────
    out.push_str("## Output File\n\n");
    out.push_str(&format!("`{output_path}`\n\n"));
    out.push_str("---\n\n");

    // ── TypeScript scaffold ───────────────────────────────────────────────────
    out.push_str("## TypeScript Scaffold\n\n");
    out.push_str("Write this exact file structure. Replace the `// IMPLEMENT` block with a real implementation:\n\n");
    out.push_str("```typescript\n");
    out.push_str(&format!("// {output_path}\n"));
    out.push_str("// Synthesized by claw synthesize — do not edit manually.\n\n");

    // Imports
    if tool.return_type.is_some() || !tool.arguments.is_empty() {
        let import_types = collect_custom_types(tool);
        if !import_types.is_empty() {
            out.push_str(&format!(
                "import type {{ {} }} from '../types.js';\n\n",
                import_types.join(", ")
            ));
        }
    }

    // Add capability-specific imports
    if let Some(using) = &tool.using {
        let cap_imports = capability_imports(using);
        if !cap_imports.is_empty() {
            out.push_str(&cap_imports);
            out.push('\n');
        }
    }

    // Secrets check block
    if !tool.secrets.is_empty() {
        out.push_str("// Runtime secret validation\n");
        for key in &tool.secrets {
            out.push_str(&format!(
                "const _{key} = process.env['{key}'];\n\
                 if (!_{key}) throw new Error('{key} env var is not set');\n"
            ));
        }
        out.push('\n');
    }

    // Function signature
    out.push_str(&format!(
        "export async function {tool_name}(args: {input_type_ts}): Promise<{return_ts}> {{\n"
    ));

    // Arg destructuring
    if !tool.arguments.is_empty() {
        let arg_names: Vec<_> = tool.arguments.iter().map(|a| a.name.as_str()).collect();
        out.push_str(&format!("  const {{ {} }} = args;\n\n", arg_names.join(", ")));
    }

    // Implementation placeholder with capability hint
    if let Some(using) = &tool.using {
        out.push_str(&capability_inline_hint(using, tool, document));
    } else {
        out.push_str("  // IMPLEMENT\n  throw new Error('not implemented');\n");
    }

    out.push_str("}\n");
    out.push_str("```\n\n");
    out.push_str("---\n\n");

    // ── Inlined type definitions ───────────────────────────────────────────────
    let custom_types = collect_all_referenced_types(tool, document);
    if !custom_types.is_empty() {
        out.push_str("## Type Definitions (exact — do not change field names or types)\n\n");
        out.push_str("```typescript\n");
        for type_name in &custom_types {
            if let Some(type_decl) = document.types.iter().find(|t| &t.name == type_name) {
                out.push_str(&format!("interface {type_name} {{\n"));
                for field in &type_decl.fields {
                    let ts_type = data_type_to_ts(&field.data_type);
                    let constraint_comment = format_constraint_comment(&field.constraints);
                    out.push_str(&format!(
                        "  {}: {};{}\n",
                        field.name, ts_type, constraint_comment
                    ));
                }
                out.push_str("}\n\n");
            }
        }
        out.push_str("```\n\n");
        out.push_str("---\n\n");
    }

    // ── Capability research ───────────────────────────────────────────────────
    if let Some(using) = &tool.using {
        out.push_str(&format!(
            "## Capability: `{}` — Implementation Patterns\n\n",
            describe_using(using)
        ));
        out.push_str(&capability_research(using, tool, document));
        out.push_str("---\n\n");
    }

    // ── Secrets ───────────────────────────────────────────────────────────────
    if !tool.secrets.is_empty() {
        out.push_str("## Required Secrets\n\n");
        out.push_str("Read from `process.env` at module load time. Never hardcode.\n\n");
        for key in &tool.secrets {
            out.push_str(&format!("- `process.env.{key}` — must be non-empty at runtime\n"));
        }
        out.push_str("\n---\n\n");
    }

    // ── Acceptance checklist ──────────────────────────────────────────────────
    out.push_str("## Acceptance Criteria\n\n");
    out.push_str("ALL of the following must be true before outputting the sentinel:\n\n");

    if let Some(tb) = &tool.test_block {
        out.push_str(&format!(
            "**Test input:** `{}`\n\n",
            tb.input.iter()
                .map(|(k, v)| format!("{k} = {}", format_expr_value(&v.expr)))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        for (field, op) in &tb.expect {
            out.push_str(&format!("- [ ] `result.{}` {}\n", field, describe_expect_op(op)));
        }
        out.push('\n');
    }

    out.push_str(&format!(
        "- [ ] File `{output_path}` is written and non-empty\n"
    ));
    out.push_str(&format!(
        "- [ ] TypeScript compiles: `tsc --noEmit {output_path}`\n"
    ));
    out.push_str("- [ ] No `eval()`, no hardcoded secrets, no `process.exit()`\n");
    out.push_str(&format!(
        "- [ ] Export name is exactly `{tool_name}` (not default export, named export)\n"
    ));
    out.push_str(&format!(
        "- [ ] Function signature matches: `({args_sig}) -> {}`\n\n",
        format_return_type(tool.return_type.as_ref())
    ));
    out.push_str("---\n\n");

    // ── Final instruction ─────────────────────────────────────────────────────
    out.push_str("## Final Step\n\n");
    out.push_str(&format!(
        "1. Write the complete implementation to `{output_path}`\n"
    ));
    out.push_str("2. Verify all acceptance criteria above are met\n");
    out.push_str("3. Output this exact text as your final line (nothing after it):\n\n");
    out.push_str(&format!("```\nSYNTHESIS_COMPLETE: {tool_name}\n```\n"));

    out
}

// ── Signature helpers ──────────────────────────────────────────────────────────

fn format_args_signature(tool: &ToolDecl) -> String {
    tool.arguments
        .iter()
        .map(|arg| format!("{}: {}", arg.name, describe_data_type(&arg.data_type)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// DSL type name (for claw source display)
fn describe_data_type(dt: &DataType) -> String {
    match dt {
        DataType::String(_) => "string".to_owned(),
        DataType::Int(_) => "int".to_owned(),
        DataType::Float(_) => "float".to_owned(),
        DataType::Boolean(_) => "bool".to_owned(),
        DataType::List(inner, _) => format!("list<{}>", describe_data_type(inner)),
        DataType::Custom(name, _) => name.clone(),
    }
}

/// TypeScript type name (for emitted TS code)
fn data_type_to_ts(dt: &DataType) -> String {
    match dt {
        DataType::String(_) => "string".to_owned(),
        DataType::Int(_) => "number".to_owned(),
        DataType::Float(_) => "number".to_owned(),
        DataType::Boolean(_) => "boolean".to_owned(),
        DataType::List(inner, _) => format!("{}[]", data_type_to_ts(inner)),
        DataType::Custom(name, _) => name.clone(),
    }
}

fn format_return_type(rt: Option<&DataType>) -> String {
    match rt {
        Some(dt) => describe_data_type(dt),
        None => "void".to_owned(),
    }
}

fn format_return_type_ts(rt: Option<&DataType>) -> String {
    match rt {
        Some(dt) => data_type_to_ts(dt),
        None => "void".to_owned(),
    }
}

/// Emits `{ field: type, ... }` for the function args parameter
fn format_input_type_ts(tool: &ToolDecl) -> String {
    if tool.arguments.is_empty() {
        return "Record<string, never>".to_owned();
    }
    let fields: Vec<_> = tool
        .arguments
        .iter()
        .map(|a| format!("{}: {}", a.name, data_type_to_ts(&a.data_type)))
        .collect();
    format!("{{ {} }}", fields.join("; "))
}

/// Collect custom type names referenced in tool signature (for import statement)
fn collect_custom_types(tool: &ToolDecl) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(DataType::Custom(name, _)) = &tool.return_type {
        names.push(name.clone());
    }
    for arg in &tool.arguments {
        if let DataType::Custom(name, _) = &arg.data_type {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
    }
    names
}

/// Collect all custom type names for inlining in the spec (including nested)
fn collect_all_referenced_types(tool: &ToolDecl, document: &Document) -> Vec<String> {
    let mut seen = Vec::new();
    let roots = collect_custom_types(tool);
    for root in roots {
        collect_type_recursive(&root, document, &mut seen);
    }
    seen
}

fn collect_type_recursive(name: &str, document: &Document, seen: &mut Vec<String>) {
    if seen.iter().any(|s: &String| s == name) {
        return;
    }
    seen.push(name.to_owned());
    if let Some(type_decl) = document.types.iter().find(|t| t.name == name) {
        for field in &type_decl.fields {
            if let DataType::Custom(nested, _) = &field.data_type {
                collect_type_recursive(nested, document, seen);
            }
        }
    }
}

fn format_constraint_comment(constraints: &[crate::ast::Constraint]) -> String {
    if constraints.is_empty() {
        return String::new();
    }
    let parts: Vec<_> = constraints
        .iter()
        .map(|c| match c.name.as_str() {
            "min" => format!("min={}", format_expr_value(&c.value.expr)),
            "max" => format!("max={}", format_expr_value(&c.value.expr)),
            "regex" => format!("regex={}", format_expr_value(&c.value.expr)),
            other => format!("{other}={}", format_expr_value(&c.value.expr)),
        })
        .collect();
    format!("  // {}", parts.join(", "))
}

fn describe_using(using: &UsingExpr) -> String {
    match using {
        UsingExpr::Fetch => "fetch".to_owned(),
        UsingExpr::Playwright => "playwright".to_owned(),
        UsingExpr::Bash => "bash".to_owned(),
        UsingExpr::Mcp(name) => format!("mcp:{name}"),
        UsingExpr::Baml(name) => format!("baml:{name}"),
    }
}

// ── Capability scaffold helpers ────────────────────────────────────────────────

/// Extra import lines at the top of the scaffold for the capability
fn capability_imports(using: &UsingExpr) -> String {
    match using {
        UsingExpr::Bash => {
            "import { execFile } from 'node:child_process';\n\
             import { promisify } from 'node:util';\n\
             const _execFile = promisify(execFile);\n"
                .to_owned()
        }
        _ => String::new(),
    }
}

/// Inline hint comment block inside the function body
fn capability_inline_hint(using: &UsingExpr, _tool: &ToolDecl, _document: &Document) -> String {
    match using {
        UsingExpr::Fetch => {
            format!(
                "  // IMPLEMENT using fetch()\n\
                 \n\
                 \n  throw new Error('not implemented');\n"
            )
        }
        UsingExpr::Playwright => {
            format!(
                "  // IMPLEMENT using playwright (see patterns below)\n\
                 \n\
                 \n  throw new Error('not implemented');\n"
            )
        }
        UsingExpr::Bash => {
            format!(
                "  // IMPLEMENT using _execFile (pre-imported above)\n\
                 \n\
                 \n  throw new Error('not implemented');\n"
            )
        }
        UsingExpr::Mcp(server) => {
            format!(
                "  // IMPLEMENT: invoke MCP tool from server `{server}`\n\
                 \n\
                 \n  throw new Error('not implemented');\n"
            )
        }
        UsingExpr::Baml(func) => {
            format!(
                "  // IMPLEMENT: call BAML function `{func}`\n\
                 \n\
                 \n  throw new Error('not implemented');\n"
            )
        }
    }
}

/// Deep implementation research per capability — actual patterns and gotchas
fn capability_research(using: &UsingExpr, tool: &ToolDecl, document: &Document) -> String {
    let return_ts = format_return_type_ts(tool.return_type.as_ref());

    // Build field list for the return type (used in all patterns)
    let return_fields: Vec<(String, String)> = tool.return_type.as_ref()
        .and_then(|rt| if let DataType::Custom(name, _) = rt {
            document.types.iter().find(|t| &t.name == name)
        } else { None })
        .map(|td| td.fields.iter().map(|f| (f.name.clone(), data_type_to_ts(&f.data_type))).collect())
        .unwrap_or_default();

    let return_shape = if return_fields.is_empty() {
        return_ts.clone()
    } else {
        let fields: Vec<_> = return_fields.iter()
            .map(|(name, ts_type)| format!("{name}: <{ts_type} value>"))
            .collect();
        format!("{{ {} }}", fields.join(", "))
    };

    match using {
        UsingExpr::Fetch => format!(
            r#"Use the Node.js built-in `fetch` (Node >= 18). No npm packages needed.

**Pattern A — JSON API:**
```typescript
const res = await fetch(`https://api.example.com/endpoint?q=${{encodeURIComponent(query)}}`, {{
  headers: {{ 'Accept': 'application/json', 'User-Agent': 'claw-tool/1.0' }},
}});
if (!res.ok) throw new Error(`HTTP ${{res.status}}: ${{await res.text()}}`);
const data = await res.json() as {{ results: Array<{{ ... }}> }};
return {return_shape};
```

**Pattern B — HTML scrape with regex:**
```typescript
const res = await fetch(url, {{ headers: {{ 'User-Agent': 'Mozilla/5.0' }} }});
const html = await res.text();
const match = html.match(/pattern/);
return {return_shape};
```

**Error handling:** Never throw on partial data — return best-effort result with empty/default fields rather than crashing the workflow.

**Rate limits:** Add `await new Promise(r => setTimeout(r, 100))` between requests if batching.

**Type assertion:** End your return with `satisfies {return_ts}` to catch shape mismatches at compile time.
"#),

        UsingExpr::Playwright => format!(
            r#"Use `playwright` for JS-rendered content. The package must be installed: `npm install playwright`.

**Full pattern:**
```typescript
import {{ chromium }} from 'playwright';

const browser = await chromium.launch({{ headless: true }});
try {{
  const context = await browser.newContext({{
    userAgent: 'Mozilla/5.0 (compatible; ClawBot/1.0)',
  }});
  const page = await context.newPage();
  await page.goto(url, {{ waitUntil: 'networkidle', timeout: 30_000 }});

  // Extract data
  const text = await page.textContent('selector') ?? '';

  return {return_shape};
}} finally {{
  await browser.close(); // ALWAYS close — prevents zombie processes
}}
```

**Selectors:** Prefer `page.locator()` over `page.$()` — it retries automatically.

**Timeouts:** Always set explicit timeouts. Default is 30s per action.

**Screenshots for debug:** `await page.screenshot({{ path: '/tmp/debug.png' }})` — useful during development.
"#),

        UsingExpr::Bash => format!(
            r#"Use `execFile` (NOT `exec`). `execFile` does not spawn a shell, preventing injection.

**Full pattern:**
```typescript
import {{ execFile }} from 'node:child_process';
import {{ promisify }} from 'node:util';
const _execFile = promisify(execFile);

// Validate inputs before use
if (!input || /[;&|`$]/.test(input)) {{
  throw new Error('invalid input characters');
}}

const {{ stdout, stderr }} = await _execFile('command', [arg1, arg2], {{
  timeout: 30_000,
  maxBuffer: 10 * 1024 * 1024, // 10MB
}});

return {return_shape};
```

**Security rules:**
- Never use `exec()` or `spawn({{ shell: true }})`
- Always validate and sanitize arguments
- Never interpolate user input into command strings
- Set explicit timeouts and buffer limits

**Parsing stdout:** Use `stdout.trim().split('\n')` for line-by-line, or `JSON.parse(stdout)` if the command outputs JSON.
"#),

        UsingExpr::Mcp(server) => format!(
            r#"Invoke the `{server}` MCP tool via the MCP client provided in the project.

**Pattern:**
```typescript
// The MCP client is available via the runtime's tool registry.
// Tool is invoked with the declared argument shape.
// Return the result mapped to {return_ts}.
```

Refer to `generated/mcp-server.js` for the tool registration to understand the available MCP methods.
"#),

        UsingExpr::Baml(func) => format!(
            r#"Call the BAML function `{func}` using the generated BAML client.

**Pattern:**
```typescript
import {{ b }} from '../baml_client/index.js';

const result = await b.{func}({{
  // match the BAML function's input schema
}});

return result satisfies {return_ts};
```

**Type safety:** The BAML client is fully typed. The `satisfies` keyword catches any shape mismatch at compile time without changing the runtime value.

**Error handling:** BAML functions throw on LLM errors. Wrap in try/catch and return a partial result if the workflow must continue on failure.
"#),
    }
}

// ── Expect / expr helpers ──────────────────────────────────────────────────────

fn describe_expect_op(op: &ExpectOp) -> String {
    match op {
        ExpectOp::NotEmpty => "is non-empty (truthy, length > 0)".to_owned(),
        ExpectOp::Gt(n) => format!("is > {n}"),
        ExpectOp::Lt(n) => format!("is < {n}"),
        ExpectOp::Gte(n) => format!("is >= {n}"),
        ExpectOp::Lte(n) => format!("is <= {n}"),
        ExpectOp::Eq(expr) => format!("equals `{}`", format_expr_value(&expr.expr)),
        ExpectOp::Matches(pat) => format!("matches regex `/{pat}/`"),
    }
}

fn format_expr_value(expr: &Expr) -> String {
    match expr {
        Expr::StringLiteral(s) => format!("\"{s}\""),
        Expr::IntLiteral(n) => n.to_string(),
        Expr::FloatLiteral(f) => f.to_string(),
        Expr::BoolLiteral(b) => b.to_string(),
        _ => "<expr>".to_owned(),
    }
}
