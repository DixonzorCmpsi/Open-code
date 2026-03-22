use std::fs;
use std::path::Path;

use serde_json::{json, Value};

use crate::ast::{
    Block, Constraint, DataType, Document, ElseBranch, Expr, ExpectOp, SpannedExpr, Statement,
    TestBlock, UsingExpr,
};
use crate::errors::{CompilerError, CompilerResult};

const CLAW_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn generate(document: &Document, project_root: &Path, source_path: &str) -> CompilerResult<()> {
    let artifact = build_artifact(document, source_path);

    let gen_dir = project_root.join("generated");
    fs::create_dir_all(&gen_dir).map_err(|e| CompilerError::IoError {
        message: format!("failed to create generated directory: {e}"),
        span: 0..0,
    })?;

    let json = serde_json::to_string_pretty(&artifact).map_err(|e| CompilerError::CodegenError {
        message: format!("failed to serialize artifact: {e}"),
        span: 0..0,
    })?;

    fs::write(gen_dir.join("artifact.clawa.json"), json).map_err(|e| CompilerError::IoError {
        message: format!("failed to write artifact: {e}"),
        span: 0..0,
    })?;

    Ok(())
}

fn build_artifact(document: &Document, source_path: &str) -> Value {
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let source_hash = compute_source_hash(document);

    json!({
        "manifest": {
            "claw_version": CLAW_VERSION,
            "source": source_path,
            "source_hash": format!("sha256:{source_hash}"),
            "generated_at": now,
        },
        "types": document.types.iter().map(emit_type_decl).collect::<Vec<_>>(),
        "tools": document.tools.iter()
            .filter(|t| t.using.is_some())
            .map(emit_tool)
            .collect::<Vec<_>>(),
        "agents": document.agents.iter().map(emit_agent).collect::<Vec<_>>(),
        "workflows": document.workflows.iter().map(emit_workflow).collect::<Vec<_>>(),
        "synthesizers": document.synthesizers.iter().map(emit_synthesizer).collect::<Vec<_>>(),
        "capability_registry": capability_registry(),
    })
}

fn compute_source_hash(document: &Document) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let canonical = format!("{document:?}");
    let digest = Sha256::digest(canonical.as_bytes());
    digest.iter().fold(String::with_capacity(64), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn emit_type_decl(t: &crate::ast::TypeDecl) -> Value {
    json!({
        "name": t.name,
        "fields": t.fields.iter().map(emit_field).collect::<Vec<_>>(),
    })
}

fn emit_field(f: &crate::ast::TypeField) -> Value {
    let mut obj = json!({
        "name": f.name,
        "type": type_name(&f.data_type),
    });

    if !f.constraints.is_empty() {
        obj["constraints"] = Value::Array(f.constraints.iter().map(emit_constraint).collect());
    }

    obj
}

fn emit_constraint(c: &Constraint) -> Value {
    json!({ &c.name: expr_to_json(&c.value) })
}

fn type_name(dt: &DataType) -> String {
    match dt {
        DataType::String(_) => "string".to_owned(),
        DataType::Int(_) => "int".to_owned(),
        DataType::Float(_) => "float".to_owned(),
        DataType::Boolean(_) => "boolean".to_owned(),
        DataType::List(inner, _) => format!("{}[]", type_name(inner)),
        DataType::Custom(name, _) => name.clone(),
    }
}

fn emit_tool(t: &crate::ast::ToolDecl) -> Value {
    let using_str = t.using.as_ref().map(using_expr_str).unwrap_or_default();

    let mut obj = json!({
        "name": t.name,
        "inputs": t.arguments.iter().map(emit_field).collect::<Vec<_>>(),
        "output_type": t.return_type.as_ref().map(type_name).unwrap_or_else(|| "void".to_owned()),
        "using": using_str,
    });

    if let Some(synth) = &t.synthesizer {
        obj["synthesizer"] = json!(synth);
    }

    if let Some(tb) = &t.test_block {
        obj["tests"] = emit_test_block(tb, &t.arguments);
    }

    obj
}

fn using_expr_str(u: &UsingExpr) -> String {
    match u {
        UsingExpr::Fetch => "fetch".to_owned(),
        UsingExpr::Playwright => "playwright".to_owned(),
        UsingExpr::Bash => "bash".to_owned(),
        UsingExpr::Mcp(name) => format!("mcp({name})"),
        UsingExpr::Baml(name) => format!("baml({name})"),
    }
}

fn emit_test_block(tb: &TestBlock, _inputs: &[crate::ast::TypeField]) -> Value {
    let input_obj: serde_json::Map<String, Value> = tb.input.iter()
        .map(|(key, val)| (key.clone(), expr_to_json(val)))
        .collect();

    let expect_obj: serde_json::Map<String, Value> = tb.expect.iter()
        .map(|(field, op)| (field.clone(), emit_expect_op(op)))
        .collect();

    json!([{
        "input": input_obj,
        "expect": expect_obj,
    }])
}

fn emit_expect_op(op: &ExpectOp) -> Value {
    match op {
        ExpectOp::NotEmpty => json!({ "op": "!empty" }),
        ExpectOp::Gt(n) => json!({ "op": "gt", "value": n }),
        ExpectOp::Lt(n) => json!({ "op": "lt", "value": n }),
        ExpectOp::Gte(n) => json!({ "op": "gte", "value": n }),
        ExpectOp::Lte(n) => json!({ "op": "lte", "value": n }),
        ExpectOp::Eq(expr) => json!({ "op": "eq", "value": expr_to_json(expr) }),
        ExpectOp::Matches(pattern) => json!({ "op": "matches", "value": pattern }),
    }
}

fn emit_agent(a: &crate::ast::AgentDecl) -> Value {
    json!({
        "name": a.name,
        "system_prompt": a.system_prompt,
        "tools": a.tools,
        "dynamic_reasoning": a.dynamic_reasoning.get(),
    })
}

fn emit_workflow(w: &crate::ast::WorkflowDecl) -> Value {
    json!({
        "name": w.name,
        "inputs": w.arguments.iter().map(emit_field).collect::<Vec<_>>(),
        "output_type": w.return_type.as_ref().map(type_name).unwrap_or_else(|| "void".to_owned()),
        "steps": emit_block_steps(&w.body),
    })
}

fn emit_block_steps(block: &Block) -> Vec<Value> {
    let mut steps = Vec::new();
    for stmt in &block.statements {
        emit_statement_steps(stmt, &mut steps);
    }
    steps
}

fn emit_statement_steps(stmt: &Statement, steps: &mut Vec<Value>) {
    match stmt {
        Statement::LetDecl { name, value, .. } => {
            match &value.expr {
                Expr::ExecuteRun { agent_name, kwargs, require_type } => {
                    steps.push(json!({
                        "kind": "tool_call",
                        "tool": agent_name,
                        "args": emit_kwargs(kwargs),
                        "bind": name,
                        "require_type": require_type.as_ref().map(type_name),
                    }));
                }
                _ => {
                    steps.push(json!({
                        "kind": "let",
                        "name": name,
                        "value": emit_expr_step(&value.expr),
                    }));
                }
            }
        }
        Statement::Return { value, .. } => {
            steps.push(json!({
                "kind": "return",
                "value": emit_expr_step(&value.expr),
            }));
        }
        Statement::Reason { using_agent, input, goal, output_type, bind, .. } => {
            steps.push(json!({
                "kind": "reason",
                "agent": using_agent,
                "input": input,
                "goal": goal,
                "output_type": type_name(output_type),
                "bind": bind,
            }));
        }
        Statement::ExecuteRun { agent_name, kwargs, require_type, .. } => {
            steps.push(json!({
                "kind": "tool_call",
                "tool": agent_name,
                "args": emit_kwargs(kwargs),
                "require_type": require_type.as_ref().map(type_name),
            }));
        }
        Statement::IfCond { condition, if_body, else_body, .. } => {
            let mut step = json!({
                "kind": "if",
                "condition": emit_expr_step(&condition.expr),
                "then": emit_block_steps(if_body),
            });
            if let Some(else_branch) = else_body {
                step["else"] = match else_branch {
                    ElseBranch::Else(block) => Value::Array(emit_block_steps(block)),
                    ElseBranch::ElseIf(stmt) => {
                        let mut nested = Vec::new();
                        emit_statement_steps(stmt, &mut nested);
                        Value::Array(nested)
                    }
                };
            }
            steps.push(step);
        }
        Statement::ForLoop { item_name, iterator, body, .. } => {
            steps.push(json!({
                "kind": "for",
                "item": item_name,
                "iterator": emit_expr_step(&iterator.expr),
                "body": emit_block_steps(body),
            }));
        }
        Statement::TryCatch { try_body, catch_name, catch_type, catch_body, .. } => {
            steps.push(json!({
                "kind": "try_catch",
                "try": emit_block_steps(try_body),
                "catch_name": catch_name,
                "catch_type": type_name(catch_type),
                "catch": emit_block_steps(catch_body),
            }));
        }
        Statement::Assert { condition, message, .. } => {
            steps.push(json!({
                "kind": "assert",
                "condition": emit_expr_step(&condition.expr),
                "message": message,
            }));
        }
        Statement::Continue(_) => steps.push(json!({ "kind": "continue" })),
        Statement::Break(_) => steps.push(json!({ "kind": "break" })),
        Statement::Expression(spanned) => {
            steps.push(json!({
                "kind": "expression",
                "value": emit_expr_step(&spanned.expr),
            }));
        }
    }
}

fn emit_kwargs(kwargs: &[(String, SpannedExpr)]) -> Value {
    let map: serde_json::Map<String, Value> = kwargs.iter()
        .map(|(k, v)| (k.clone(), emit_arg_value(&v.expr)))
        .collect();
    Value::Object(map)
}

fn emit_arg_value(expr: &Expr) -> Value {
    match expr {
        Expr::Identifier(name) => json!({ "ref": name }),
        Expr::StringLiteral(s) if s.contains("${") => json!({ "interpolate": s }),
        _ => emit_expr_step(expr),
    }
}

fn emit_expr_step(expr: &Expr) -> Value {
    match expr {
        Expr::StringLiteral(s) => json!(s),
        Expr::IntLiteral(n) => json!(n),
        Expr::FloatLiteral(f) => json!(f),
        Expr::BoolLiteral(b) => json!(b),
        Expr::Identifier(name) => json!({ "ref": name }),
        Expr::ArrayLiteral(items) => {
            Value::Array(items.iter().map(|i| emit_expr_step(&i.expr)).collect())
        }
        Expr::MemberAccess(obj, field) => {
            json!({ "member": emit_expr_step(&obj.expr), "field": field })
        }
        Expr::Call(name, args) => {
            json!({
                "call": name,
                "args": args.iter().map(|a| emit_expr_step(&a.expr)).collect::<Vec<_>>(),
            })
        }
        Expr::MethodCall(obj, method, args) => {
            json!({
                "method_call": method,
                "receiver": emit_expr_step(&obj.expr),
                "args": args.iter().map(|a| emit_expr_step(&a.expr)).collect::<Vec<_>>(),
            })
        }
        Expr::ExecuteRun { agent_name, kwargs, require_type } => {
            json!({
                "tool_call": agent_name,
                "args": emit_kwargs(kwargs),
                "require_type": require_type.as_ref().map(type_name),
            })
        }
        Expr::DirectToolCall { tool_name, args } => {
            json!({
                "direct_tool_call": tool_name,
                "args": emit_kwargs(args),
            })
        }
        Expr::BinaryOp { left, op, right } => {
            json!({
                "binary_op": format!("{op:?}"),
                "left": emit_expr_step(&left.expr),
                "right": emit_expr_step(&right.expr),
            })
        }
    }
}

fn expr_to_json(expr: &SpannedExpr) -> Value {
    match &expr.expr {
        Expr::StringLiteral(s) => json!(s),
        Expr::IntLiteral(n) => json!(n),
        Expr::FloatLiteral(f) => json!(f),
        Expr::BoolLiteral(b) => json!(b),
        _ => json!(null),
    }
}

fn emit_synthesizer(s: &crate::ast::SynthesizerDecl) -> Value {
    // Determine provider and model from the client name. The SynthesizerDecl
    // just stores the client reference; we emit what we know at this level.
    json!({
        "name": s.name,
        "client": s.client,
        "temperature": s.temperature,
        "max_tokens": s.max_tokens,
    })
}

fn capability_registry() -> Value {
    json!({
        "fetch": {
            "runtime":       "node-fetch",
            "import":        "import fetch from 'node-fetch';",
            "pattern":       "async function(inputs: T): Promise<U>",
            "mock_strategy": "intercept_fetch"
        },
        "playwright": {
            "runtime":       "@playwright/test",
            "import":        "import { chromium } from 'playwright';",
            "pattern":       "async function(inputs: T): Promise<U>",
            "mock_strategy": "skip_in_unit_tests"
        },
        "mcp": {
            "runtime":       "@modelcontextprotocol/sdk",
            "pattern":       "async function(inputs: T): Promise<U>",
            "mock_strategy": "mock_client"
        },
        "baml": {
            "runtime":       "@boundaryml/baml",
            "pattern":       "async function(inputs: T): Promise<U>",
            "mock_strategy": "mock_baml_client"
        },
        "bash": {
            "runtime":       "node:child_process",
            "import":        "import { exec } from 'node:child_process';",
            "pattern":       "async function(inputs: T): Promise<U>",
            "mock_strategy": "mock_exec"
        }
    })
}
