use std::collections::HashMap;

use crate::ast::{ClientDecl, Constraint, DataType, Document, Expr, TypeDecl};
use crate::errors::CompilerResult;

// ─── Public IR types ────────────────────────────────────────────────────────

/// A fully resolved agent with inherited properties materialized from extends chain.

pub struct BamlOutput {
    pub generators: String,
    pub clients: String,
    pub types: String,
    pub functions: String,
}

pub fn generate_baml(
    document: &Document,
) -> CompilerResult<BamlOutput> {
    let baml_tools = collect_baml_tools(document);
    // Use the first declared client's name as the BAML default client; fall back to "DefaultClient"
    let default_client = document.clients.first().map(|c| c.name.as_str()).unwrap_or("DefaultClient");
    Ok(BamlOutput {
        generators: emit_generators(),
        clients: emit_clients(&document.clients),
        types: emit_types(&document.types)?,
        functions: emit_functions(&baml_tools, default_client),
    })
}

pub fn collect_baml_tools(document: &Document) -> Vec<&crate::ast::ToolDecl> {
    document.tools.iter()
        .filter(|t| t.invoke_path.as_deref().unwrap_or("").starts_with("baml("))
        .collect()
}

// ─── Generators block ────────────────────────────────────────────────────────

fn emit_generators() -> String {
    concat!(
        "generator target {\n",
        "  output_type \"typescript\"\n",
        "  output_dir \"../baml_client\"\n",
        "  version \"0.70.0\"\n",
        "}\n",
    )
    .to_owned()
}

// ─── Clients block ───────────────────────────────────────────────────────────

fn emit_clients(clients: &[ClientDecl]) -> String {
    let mut out = String::new();
    for client in clients {
        let provider = match client.provider.as_str() {
            "openai" => "openai",
            "anthropic" => "anthropic",
            other => other,
        };
        out.push_str(&format!("client<llm> {} {{\n", client.name));
        out.push_str(&format!("  provider {}\n", provider));
        out.push_str("  options {\n");
        out.push_str(&format!("    model \"{}\"\n", client.model));
        if let Some(retries) = client.retries {
            out.push_str(&format!("    max_retries {}\n", retries));
        }
        out.push_str("  }\n");
        out.push_str("}\n\n");
    }
    out
}

// ─── Types block ─────────────────────────────────────────────────────────────

fn emit_types(types: &[TypeDecl]) -> CompilerResult<String> {
    let mut out = String::new();
    for type_decl in topological_sort(types) {
        emit_class(&mut out, type_decl);
        out.push('\n');
    }
    Ok(out)
}

fn topological_sort(types: &[TypeDecl]) -> Vec<&TypeDecl> {
    let name_to_idx: HashMap<&str, usize> =
        types.iter().enumerate().map(|(i, t)| (t.name.as_str(), i)).collect();

    let mut visited = vec![false; types.len()];
    let mut result: Vec<&TypeDecl> = Vec::with_capacity(types.len());

    for i in 0..types.len() {
        topo_visit(i, types, &name_to_idx, &mut visited, &mut result);
    }
    result
}

fn topo_visit<'a>(
    idx: usize,
    types: &'a [TypeDecl],
    name_to_idx: &HashMap<&str, usize>,
    visited: &mut Vec<bool>,
    result: &mut Vec<&'a TypeDecl>,
) {
    if visited[idx] {
        return;
    }
    visited[idx] = true;
    for field in &types[idx].fields {
        if let DataType::Custom(dep, _) = &field.data_type {
            if let Some(&dep_idx) = name_to_idx.get(dep.as_str()) {
                topo_visit(dep_idx, types, name_to_idx, visited, result);
            }
        }
    }
    result.push(&types[idx]);
}

fn emit_class(out: &mut String, type_decl: &TypeDecl) {
    out.push_str(&format!("class {} {{\n", type_decl.name));
    for field in &type_decl.fields {
        let baml_type = data_type_to_baml(&field.data_type);
        let checks = field_constraint_checks(&field.constraints);
        if checks.is_empty() {
            out.push_str(&format!("  {} {}\n", field.name, baml_type));
        } else {
            out.push_str(&format!("  {} {} {}\n", field.name, baml_type, checks));
        }
    }
    out.push_str("}\n");
}

fn data_type_to_baml(dt: &DataType) -> String {
    match dt {
        DataType::String(_) => "string".to_owned(),
        DataType::Int(_) => "int".to_owned(),
        DataType::Float(_) => "float".to_owned(),
        DataType::Boolean(_) => "bool".to_owned(),
        DataType::List(inner, _) => format!("{}[]", data_type_to_baml(inner)),
        DataType::Custom(name, _) => name.clone(),
    }
}

fn field_constraint_checks(constraints: &[Constraint]) -> String {
    let checks: Vec<String> = constraints
        .iter()
        .filter_map(|c| constraint_to_check(&c.name, &c.value.expr))
        .collect();
    checks.join(" ")
}

fn constraint_to_check(name: &str, value: &Expr) -> Option<String> {
    let val_str = match value {
        Expr::IntLiteral(n) => n.to_string(),
        Expr::FloatLiteral(n) => n.to_string(),
        Expr::StringLiteral(s) => format!("\"{}\"", s),
        Expr::BoolLiteral(b) => b.to_string(),
        _ => return None,
    };
    let expr = match name {
        "min" => format!("this >= {}", val_str),
        "max" => format!("this <= {}", val_str),
        "min_length" => format!("this.length >= {}", val_str),
        "max_length" => format!("this.length <= {}", val_str),
        "regex" => format!("this matches {}", val_str),
        _ => return None,
    };
    Some(format!("@check({}, {{{{ {} }}}})", name, expr))
}

// ─── Functions block ──────────────────────────────────────────────────────────

fn emit_functions(tools: &[&crate::ast::ToolDecl], default_client: &str) -> String {
    let mut out = String::new();
    for tool in tools {
        emit_function(&mut out, tool, default_client);
        out.push('\n');
    }
    out
}

fn emit_function(out: &mut String, tool: &crate::ast::ToolDecl, default_client: &str) {
    let return_type_baml = tool.return_type.as_ref()
        .map(|dt| data_type_to_baml(dt))
        .unwrap_or_else(|| "string".to_owned());
    
    // Parse "baml(MyFunction)" into "MyFunction"
    let invoke_path = tool.invoke_path.as_deref().unwrap_or("");
    let baml_func_name = if invoke_path.starts_with("baml(") && invoke_path.ends_with(")") {
        let inside = &invoke_path[5..invoke_path.len()-1];
        // Strip quotes if present
        inside.trim_matches('"').to_owned()
    } else {
        tool.name.clone()
    };

    if tool.arguments.is_empty() {
        out.push_str(&format!("function {} -> {} {{\n", baml_func_name, return_type_baml));
    } else {
        out.push_str(&format!("function {}(\n", baml_func_name));
        for (i, field) in tool.arguments.iter().enumerate() {
            let comma = if i + 1 < tool.arguments.len() { "," } else { "" };
            out.push_str(&format!("  {}: {}{}\n", field.name, data_type_to_baml(&field.data_type), comma));
        }
        out.push_str(&format!(") -> {} {{\n", return_type_baml));
    }

    out.push_str(&format!("  client {}\n", default_client));
    out.push_str("  prompt #\"\n");
    out.push_str("    {{ _.role(\"system\") }}\n");
    out.push_str("    Extract information accurately according to the schema.\n\n");
    out.push_str("    {{ _.role(\"user\") }}\n");
    
    for field in &tool.arguments {
        out.push_str(&format!("    {}: {{{{ {} }}}}\n", field.name, field.name));
    }
    
    out.push_str("  \"#\n");
    out.push_str("}\n");
}
