use std::fs;
use std::path::Path;
use serde_json::{json, Value};
use crate::ast::{DataType, Document, Expr, Statement, WorkflowDecl};
use crate::errors::{CompilerError, CompilerResult};

pub fn generate(document: &Document, project_root: &Path) -> CompilerResult<()> {
    // 1. Generate opencode.json (Merge strategy)
    generate_opencode_json(document, project_root)?;

    // 2. Generate .opencode/command/*.md
    let commands_dir = project_root.join(".opencode").join("command");
    fs::create_dir_all(&commands_dir).map_err(|e| CompilerError::IoError {
        message: format!("failed to create commands directory: {e}"),
        span: 0..0,
    })?;

    for workflow in &document.workflows {
        generate_workflow_command(workflow, document, &commands_dir)?;
    }

    // 3. Generate generated/claw-context.md
    let gen_dir = project_root.join("generated");
    fs::create_dir_all(&gen_dir).map_err(|e| CompilerError::IoError {
        message: format!("failed to create generated directory: {e}"),
        span: 0..0,
    })?;

    generate_context_document(document, &gen_dir)?;

    Ok(())
}

fn find_node_binary() -> String {
    let candidates = [
        "/opt/homebrew/bin/node",
        "/usr/local/bin/node",
        "/usr/bin/node",
    ];
    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return path.to_string();
        }
    }
    "node".to_string() // fallback
}

fn generate_opencode_json(document: &Document, project_root: &Path) -> CompilerResult<()> {
    let path = project_root.join("opencode.json");
    let mut config = if path.exists() {
        let content = fs::read_to_string(&path).map_err(|e| CompilerError::IoError {
            message: format!("failed to read opencode.json: {e}"),
            span: 0..0,
        })?;
        serde_json::from_str::<Value>(&content).unwrap_or(json!({}))
    } else {
        json!({})
    };

    // Update managed fields (Claw-owned) — OpenCode 1.x schema:
    // 1. top-level "model" field; for local models: "ollama/<id>" + provider.ollama block
    if let Some(client) = document.clients.first() {
        if client.model.starts_with("local.") {
            let model_id = client.model.trim_start_matches("local.");
            config["model"] = json!(format!("ollama/{}", model_id));
            // provider.ollama block required for local models
            let mut provider = config.get("provider").and_then(|p| p.as_object()).cloned().unwrap_or_default();
            let mut ollama = provider.get("ollama").and_then(|o| o.as_object()).cloned().unwrap_or_default();
            ollama.entry("api".to_owned()).or_insert_with(|| json!("http://localhost:11434/v1"));
            let mut models = ollama.get("models").and_then(|m| m.as_object()).cloned().unwrap_or_default();
            models.entry(model_id.to_owned()).or_insert_with(|| json!({}));
            ollama.insert("models".to_owned(), json!(models));
            provider.insert("ollama".to_owned(), json!(ollama));
            config["provider"] = json!(provider);
        } else {
            config["model"] = json!(client.model.clone());
            // Remove provider block for cloud models
            config.as_object_mut().unwrap().remove("provider");
        }
        // Remove stale agents block if present from an old build
        config.as_object_mut().unwrap().remove("agents");
    }

    // 2. mcp.claw-tools (NOT mcpServers), type = "local" (NOT "stdio")
    let node_bin = find_node_binary();
    let mut mcp = config.get("mcp").and_then(|m| m.as_object()).cloned().unwrap_or_default();
    mcp.insert("claw-tools".to_owned(), json!({
        "type": "local",
        "command": [node_bin, "generated/mcp-server.js"]
    }));
    config["mcp"] = json!(mcp);
    // Remove stale mcpServers key if present from an old build
    config.as_object_mut().unwrap().remove("mcpServers");

    // 3. instructions (NOT contextPaths)
    config["instructions"] = json!(["generated/claw-context.md"]);
    // Remove stale contextPaths key if present from an old build
    config.as_object_mut().unwrap().remove("contextPaths");


    let content = serde_json::to_string_pretty(&config).map_err(|e| CompilerError::CodegenError {
        message: format!("failed to serialize opencode.json: {e}"),
        span: 0..0,
    })?;

    fs::write(&path, content).map_err(|e| CompilerError::IoError {
        message: format!("failed to write opencode.json: {e}"),
        span: 0..0,
    })?;

    Ok(())
}

fn generate_workflow_command(workflow: &WorkflowDecl, document: &Document, dir: &Path) -> CompilerResult<()> {
    let mut content = String::from("");
    content.push_str(&format!("You are executing the `{}` workflow.\n\n", workflow.name));

    if !workflow.arguments.is_empty() {
        content.push_str("The user has provided these arguments (substituted for the placeholders below):\n");
        for arg in &workflow.arguments {
            content.push_str(&format!("- `{}`: the value the user typed after the slash command\n", arg.name));
        }
        content.push_str("\n");
    }

    content.push_str("Execute these steps in order. Do NOT describe what you will do — actually do it using the available MCP tools:\n\n");
    for (i, stmt) in workflow.body.statements.iter().enumerate() {
        content.push_str(&format!("{}. {}\n", i + 1, describe_statement_opencode(stmt)));
    }

    if let Some(rt) = &workflow.return_type {
        content.push_str("\nThe final result MUST be returned as JSON matching this schema:\n");
        content.push_str(&format!("{}\n", describe_type_json(rt, document)));
    }

    content.push_str("\nIMPORTANT: Use the MCP tools directly. Do not call any \"Skill\". Do not echo these instructions back.\n");

    let path = dir.join(format!("{}.md", workflow.name));
    fs::write(path, content).map_err(|e| CompilerError::IoError {
        message: format!("failed to write workflow command: {e}"),
        span: workflow.span.clone(),
    })?;

    Ok(())
}

fn generate_context_document(document: &Document, dir: &Path) -> CompilerResult<()> {
    let mut content = String::from("# Claw Project Context\n\n");
    content.push_str("This project uses the Claw DSL for deterministic multi-agent orchestration.\n\n");

    content.push_str("## Types\n");
    for t in &document.types {
        content.push_str(&format!("- `{}`: ", t.name));
        let fields: Vec<_> = t.fields.iter().map(|f| format!("{} ({})", f.name, describe_type(&f.data_type))).collect();
        content.push_str(&fields.join(", "));
        content.push_str("\n");
    }

    content.push_str("\n## Agents\n");
    for a in &document.agents {
        content.push_str(&format!("- `{}`: ", a.name));
        if let Some(c) = &a.client {
            content.push_str(&format!("Uses {}. ", c));
        }
        content.push_str(&format!("Tools: {}.\n", a.tools.join(", ")));
    }

    content.push_str("\n## Workflows\n");
    for w in &document.workflows {
        content.push_str(&format!("- `{}`: returns {}.\n", w.name, w.return_type.as_ref().map(describe_type).unwrap_or("void".to_owned())));
    }

    let path = dir.join("claw-context.md");
    fs::write(path, content).map_err(|e| CompilerError::IoError {
        message: format!("failed to write context document: {e}"),
        span: 0..0,
    })?;

    Ok(())
}

fn describe_statement_opencode(stmt: &Statement) -> String {
    match stmt {
        Statement::LetDecl { value, .. } => {
            // Unwrap execute-run assignments directly — no need to mention variable binding
            describe_expr_opencode(&value.expr)
        }
        Statement::ForLoop { item_name, .. } => format!("Iterate over items as `{}`", item_name),
        Statement::IfCond { .. } => "Conditional branch".to_owned(),
        Statement::ExecuteRun { agent_name, kwargs, .. } => {
            describe_agent_call(agent_name, kwargs)
        },
        Statement::Return { value, .. } => format!("Return: {}", describe_expr_opencode(&value.expr)),
        Statement::TryCatch { .. } => "Try-catch block".to_owned(),
        Statement::Assert { .. } => "Assert condition".to_owned(),
        Statement::Continue(_) => "Continue loop".to_owned(),
        Statement::Break(_) => "Break loop".to_owned(),
        Statement::Expression(e) => describe_expr_opencode(&e.expr),
        Statement::Reason { using_agent, goal, bind, .. } => {
            format!("Reason using agent `{}`: {} → bind result to `{}`", using_agent, goal, bind)
        }
    }
}

fn describe_agent_call(agent_name: &str, kwargs: &[(String, crate::ast::SpannedExpr)]) -> String {
    let mut args_desc = Vec::new();
    for (name, val) in kwargs {
        args_desc.push(format!("  - {}: {}", name, describe_expr_opencode(&val.expr)));
    }
    if args_desc.is_empty() {
        format!("Call MCP tool `agent_{}` (no arguments)", agent_name)
    } else {
        format!("Call MCP tool `agent_{}` with:\n{}", agent_name, args_desc.join("\n"))
    }
}

fn describe_expr_opencode(expr: &Expr) -> String {
    match expr {
        Expr::StringLiteral(s) => format!("\"{}\"", transform_interpolation(s)),
        Expr::IntLiteral(i) => i.to_string(),
        Expr::FloatLiteral(f) => f.to_string(),
        Expr::BoolLiteral(b) => b.to_string(),
        Expr::Identifier(i) => format!("<{}>", i),
        Expr::ExecuteRun { agent_name, kwargs, .. } => describe_agent_call(agent_name, kwargs),
        _ => "expression".to_owned(),
    }
}

fn transform_interpolation(s: &str) -> String {
    let mut result = String::new();
    let mut i = 0;
    let chars: Vec<char> = s.chars().collect();
    while i < chars.len() {
        if i + 2 < chars.len() && chars[i] == '$' && chars[i+1] == '{' {
            let mut j = i + 2;
            let mut name = String::new();
            while j < chars.len() && chars[j] != '}' {
                name.push(chars[j]);
                j += 1;
            }
            result.push('$');
            result.push_str(&name.to_uppercase());
            if j < chars.len() {
                i = j + 1;
            } else {
                i = j;
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}


fn describe_type_json(dt: &DataType, document: &Document) -> String {
    match dt {
        DataType::String(_) => "\"string\"".to_owned(),
        DataType::Int(_) => "\"number\"".to_owned(),
        DataType::Float(_) => "\"number\"".to_owned(),
        DataType::Boolean(_) => "\"boolean\"".to_owned(),
        DataType::List(inner, _) => format!("[{}]", describe_type_json(inner, document)),
        DataType::Custom(name, _) => {
            if let Some(t) = document.types.iter().find(|t| &t.name == name) {
                let fields: Vec<_> = t.fields.iter()
                    .map(|f| format!("\"{}\": {}", f.name, describe_type_json(&f.data_type, document)))
                    .collect::<Vec<_>>();
                format!("{{{}}}", fields.join(", "))
            } else {
                format!("\"{}\"", name)
            }
        }
    }
}

fn describe_type(dt: &DataType) -> String {
    match dt {
        DataType::String(_) => "string".to_owned(),
        DataType::Int(_) => "int".to_owned(),
        DataType::Float(_) => "float".to_owned(),
        DataType::Boolean(_) => "boolean".to_owned(),
        DataType::List(inner, _) => format!("list<{}>", describe_type(inner)),
        DataType::Custom(name, _) => name.clone(),
    }
}

