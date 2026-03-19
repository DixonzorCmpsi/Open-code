use std::fs;
use std::path::{Path, PathBuf};
use serde_json::{json, Value};
use crate::ast::*;
use crate::errors::{CompilerError, CompilerResult};

pub fn generate(document: &Document, project_root: &Path) -> CompilerResult<()> {
    // 1. Generate opencode.json (Merge strategy)
    generate_opencode_json(document, project_root)?;

    // 2. Generate .opencode/commands/*.md
    let commands_dir = project_root.join(".opencode").join("commands");
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

    // Update managed fields (Claw-owned)
    // 1. agents.coder.model
    if let Some(client) = document.clients.first() {
        let mut agents = config.get("agents").and_then(|a| a.as_object()).cloned().unwrap_or_default();
        let mut coder = agents.get("coder").and_then(|c| c.as_object()).cloned().unwrap_or_default();
        coder.insert("model".to_owned(), json!(client.model));
        agents.insert("coder".to_owned(), json!(coder));
        config["agents"] = json!(agents);
    }

    // 2. mcpServers.claw-tools
    let mut mcp_servers = config.get("mcpServers").and_then(|m| m.as_object()).cloned().unwrap_or_default();
    mcp_servers.insert("claw-tools".to_owned(), json!({
        "command": "node",
        "args": ["generated/mcp-server.js"],
        "type": "stdio"
    }));
    config["mcpServers"] = json!(mcp_servers);

    // 3. contextPaths
    let mut context_paths = Vec::new();
    if project_root.join("AGENTS.md").exists() {
        context_paths.push("AGENTS.md".to_owned());
    }
    context_paths.push("generated/claw-context.md".to_owned());
    config["contextPaths"] = json!(context_paths);


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

fn generate_agent_markdown(_agent: &AgentDecl, _document: &Document, _dir: &Path) -> CompilerResult<()> {
    Ok(())
}

fn generate_workflow_command(workflow: &WorkflowDecl, document: &Document, dir: &Path) -> CompilerResult<()> {
    let mut content = String::from("");
    content.push_str(&format!("Run the {} workflow.\n\n", workflow.name));
    
    for arg in &workflow.arguments {
        content.push_str(&format!("{}: ${}\n", arg.name, arg.name.to_uppercase()));
    }
    content.push_str("\n");

    // Logic description
    content.push_str("Steps:\n");
    for (i, stmt) in workflow.body.statements.iter().enumerate() {
        content.push_str(&format!("{}. {}\n", i + 1, describe_statement_opencode(stmt)));
    }

    if let Some(rt) = &workflow.return_type {
        content.push_str("\nExpected output format (JSON):\n");
        content.push_str(&format!("{}\n", describe_type_json(rt, document)));
    }

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

fn find_primary_agent<'a>(workflow: &'a WorkflowDecl, document: &'a Document) -> Option<&'a str> {
    for stmt in &workflow.body.statements {
        if let Statement::ExecuteRun { agent_name, .. } = stmt {
            return Some(agent_name);
        }
        // Also check expressions
        if let Statement::LetDecl { value, .. } = stmt {
             if let Expr::ExecuteRun { agent_name, .. } = &value.expr {
                 return Some(agent_name);
             }
        }
    }
    None
}

fn describe_statement_opencode(stmt: &Statement) -> String {
    match stmt {
        Statement::LetDecl { name, value, .. } => format!("Initialize variable `{}` with {}", name, describe_expr_opencode(&value.expr)),
        Statement::ForLoop { item_name, .. } => format!("Iterate over items as `{}`", item_name),
        Statement::IfCond { .. } => "Conditional branch".to_owned(),
        Statement::ExecuteRun { agent_name, kwargs, .. } => {
            let mut args_desc = Vec::new();
            for (name, val) in kwargs {
                args_desc.push(format!("{}: {}", name, describe_expr_opencode(&val.expr)));
            }
            format!("Call agent_{} with {}", agent_name, args_desc.join(", "))
        },
        Statement::Return { .. } => "Return the result".to_owned(),
        Statement::TryCatch { .. } => "Try-catch block".to_owned(),
        Statement::Assert { .. } => "Assert condition".to_owned(),
        Statement::Continue(_) => "Continue loop".to_owned(),
        Statement::Break(_) => "Break loop".to_owned(),
        Statement::Expression(e) => format!("Evaluate: {}", describe_expr_opencode(&e.expr)),
    }
}

fn describe_expr_opencode(expr: &Expr) -> String {
    match expr {
        Expr::StringLiteral(s) => format!("\"{}\"", transform_interpolation(s)),
        Expr::IntLiteral(i) => i.to_string(),
        Expr::FloatLiteral(f) => f.to_string(),
        Expr::BoolLiteral(b) => b.to_string(),
        Expr::Identifier(i) => format!("${}", i.to_uppercase()),
        Expr::ExecuteRun { agent_name, .. } => format!("Call agent_{}", agent_name),
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

