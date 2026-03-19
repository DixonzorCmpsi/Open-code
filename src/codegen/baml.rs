use std::collections::{HashMap, HashSet};

use crate::ast::{AgentDecl, Block, ClientDecl, Constraint, DataType, Document, ElseBranch, Expr, Span, SpannedExpr, Statement, TypeDecl};
use crate::errors::{CompilerError, CompilerResult};

// ─── Public IR types ────────────────────────────────────────────────────────

/// A fully resolved agent with inherited properties materialized from extends chain.
pub struct ResolvedAgent {
    pub name: String,
    pub client: Option<String>,
    pub system_prompt: Option<String>,
    pub tools: Vec<String>,
    pub span: Span,
}

/// A kwarg parameter for a BAML function.
pub struct CallSiteParam {
    pub name: String,
    /// true if this kwarg does not appear in every call site for this (agent, return_type) pair
    pub is_optional: bool,
}

/// A unique call site signature — one BAML function per (agent_name, return_type_name) pair.
pub struct CallSiteSignature {
    pub agent_name: String,
    pub return_type_name: String,
    pub params: Vec<CallSiteParam>,
    pub baml_function_name: String,
}

/// The four BAML output files produced by `generate_baml`.
pub struct BamlOutput {
    pub generators: String,
    pub clients: String,
    pub types: String,
    pub functions: String,
}

// ─── Public API ─────────────────────────────────────────────────────────────

pub fn generate_baml(
    document: &Document,
    resolved_agents: &[ResolvedAgent],
    call_sites: &[CallSiteSignature],
) -> CompilerResult<BamlOutput> {
    Ok(BamlOutput {
        generators: emit_generators(),
        clients: emit_clients(&document.clients),
        types: emit_types(&document.types)?,
        functions: emit_functions(resolved_agents, call_sites),
    })
}

/// Walk the extends chain for every agent, materialising inherited client/system_prompt/tools.
pub fn resolve_agents(document: &Document) -> CompilerResult<Vec<ResolvedAgent>> {
    document
        .agents
        .iter()
        .map(|agent| resolve_one_agent(agent, document))
        .collect()
}

fn resolve_one_agent(agent: &AgentDecl, document: &Document) -> CompilerResult<ResolvedAgent> {
    let mut resolved = ResolvedAgent {
        name: agent.name.clone(),
        client: agent.client.clone(),
        system_prompt: agent.system_prompt.clone(),
        tools: agent.tools.clone(),
        span: agent.span.clone(),
    };

    let mut parent_name = agent.extends.clone();
    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(agent.name.clone());

    while let Some(ref name) = parent_name {
        if visited.contains(name) {
            return Err(CompilerError::CircularAgentExtends {
                agent_name: agent.name.clone(),
                span: agent.span.clone(),
            });
        }
        visited.insert(name.clone());

        if let Some(parent) = document.agents.iter().find(|a| &a.name == name) {
            if resolved.client.is_none() {
                resolved.client = parent.client.clone();
            }
            if resolved.system_prompt.is_none() {
                resolved.system_prompt = parent.system_prompt.clone();
            }
            // Only inherit tools if this agent declared none
            if agent.tools.is_empty() {
                resolved.tools = parent.tools.clone();
            }
            parent_name = parent.extends.clone();
        } else {
            // Semantic pass already validated extends targets exist.
            break;
        }
    }

    Ok(resolved)
}

/// Collect all execute-run call sites from every workflow and produce one
/// `CallSiteSignature` per unique `(agent_name, return_type_name)` pair.
pub fn collect_call_sites(document: &Document) -> Vec<CallSiteSignature> {
    // Map: (agent_name, return_type_name) → list of kwarg-name sets (one per call site)
    let mut sites: HashMap<(String, String), Vec<Vec<String>>> = HashMap::new();

    for workflow in &document.workflows {
        visit_block(&workflow.body, &mut sites);
    }

    let mut result: Vec<CallSiteSignature> = sites
        .into_iter()
        .map(|((agent_name, return_type_name), kwarg_sets)| {
            let all_names: HashSet<String> = kwarg_sets.iter().flatten().cloned().collect();
            let mut params: Vec<CallSiteParam> = all_names
                .into_iter()
                .map(|name| {
                    let present_in_all = kwarg_sets.iter().all(|set| set.contains(&name));
                    CallSiteParam {
                        name,
                        is_optional: !present_in_all,
                    }
                })
                .collect();
            // Deterministic order: required params first, then optional, alpha within each group
            params.sort_by(|a, b| match (a.is_optional, b.is_optional) {
                (false, true) => std::cmp::Ordering::Less,
                (true, false) => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
            });

            let baml_function_name = format!("{}Run_{}", agent_name, return_type_name);
            CallSiteSignature {
                baml_function_name,
                agent_name,
                return_type_name,
                params,
            }
        })
        .collect();

    result.sort_by(|a, b| a.baml_function_name.cmp(&b.baml_function_name));
    result
}

// ─── AST visitor ────────────────────────────────────────────────────────────

fn visit_block(block: &Block, sites: &mut HashMap<(String, String), Vec<Vec<String>>>) {
    for stmt in &block.statements {
        visit_statement(stmt, sites);
    }
}

fn visit_statement(stmt: &Statement, sites: &mut HashMap<(String, String), Vec<Vec<String>>>) {
    match stmt {
        Statement::ExecuteRun { agent_name, kwargs, require_type, .. } => {
            record_call_site(agent_name, require_type.as_ref(), kwargs.iter().map(|(k, _)| k.as_str()), sites);
        }
        Statement::LetDecl { value, .. } | Statement::Return { value, .. } => {
            visit_spanned_expr(value, sites);
        }
        Statement::Expression(spanned) => {
            visit_spanned_expr(spanned, sites);
        }
        Statement::ForLoop { body, .. } => {
            visit_block(body, sites);
        }
        Statement::IfCond { if_body, else_body, .. } => {
            visit_block(if_body, sites);
            match else_body {
                Some(ElseBranch::Else(block)) => visit_block(block, sites),
                Some(ElseBranch::ElseIf(stmt)) => visit_statement(stmt, sites),
                None => {}
            }
        }
        Statement::TryCatch { try_body, catch_body, .. } => {
            visit_block(try_body, sites);
            visit_block(catch_body, sites);
        }
        Statement::Assert { .. } | Statement::Continue(_) | Statement::Break(_) => {}
    }
}

fn visit_spanned_expr(spanned: &SpannedExpr, sites: &mut HashMap<(String, String), Vec<Vec<String>>>) {
    match &spanned.expr {
        Expr::ExecuteRun { agent_name, kwargs, require_type } => {
            record_call_site(agent_name, require_type.as_ref(), kwargs.iter().map(|(k, _)| k.as_str()), sites);
        }
        Expr::ArrayLiteral(items) => {
            for item in items {
                visit_spanned_expr(item, sites);
            }
        }
        Expr::MethodCall(target, _, args) => {
            visit_spanned_expr(target, sites);
            for arg in args {
                visit_spanned_expr(arg, sites);
            }
        }
        Expr::MemberAccess(target, _) => visit_spanned_expr(target, sites),
        Expr::BinaryOp { left, right, .. } => {
            visit_spanned_expr(left, sites);
            visit_spanned_expr(right, sites);
        }
        _ => {}
    }
}

fn record_call_site<'a>(
    agent_name: &str,
    require_type: Option<&DataType>,
    kwarg_names: impl Iterator<Item = &'a str>,
    sites: &mut HashMap<(String, String), Vec<Vec<String>>>,
) {
    let return_type_name = data_type_to_return_name(require_type);
    let kwarg_vec: Vec<String> = kwarg_names.map(str::to_owned).collect();
    sites
        .entry((agent_name.to_owned(), return_type_name))
        .or_default()
        .push(kwarg_vec);
}

fn data_type_to_return_name(dt: Option<&DataType>) -> String {
    match dt {
        Some(DataType::Custom(name, _)) => name.clone(),
        Some(DataType::String(_)) => "String".to_owned(),
        Some(DataType::Int(_)) => "Int".to_owned(),
        Some(DataType::Float(_)) => "Float".to_owned(),
        Some(DataType::Boolean(_)) => "Bool".to_owned(),
        Some(DataType::List(inner, _)) => format!("ListOf{}", data_type_to_return_name(Some(inner))),
        None => "String".to_owned(),
    }
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

fn emit_functions(resolved_agents: &[ResolvedAgent], call_sites: &[CallSiteSignature]) -> String {
    let agent_map: HashMap<&str, &ResolvedAgent> =
        resolved_agents.iter().map(|a| (a.name.as_str(), a)).collect();

    let mut out = String::new();
    for sig in call_sites {
        let agent = match agent_map.get(sig.agent_name.as_str()) {
            Some(a) => a,
            None => continue,
        };
        // Tool-using agents skip BAML function generation (spec §4.5)
        if !agent.tools.is_empty() {
            continue;
        }
        let client = agent.client.as_deref().unwrap_or("DefaultClient");
        let system_prompt = agent
            .system_prompt
            .as_deref()
            .unwrap_or("You are a helpful assistant.");

        emit_function(&mut out, sig, client, system_prompt);
        out.push('\n');
    }
    out
}

fn emit_function(out: &mut String, sig: &CallSiteSignature, client: &str, system_prompt: &str) {
    // Build parameter list
    if sig.params.is_empty() {
        out.push_str(&format!("function {} -> {} {{\n", sig.baml_function_name, sig.return_type_name));
    } else {
        out.push_str(&format!("function {}(\n", sig.baml_function_name));
        for (i, p) in sig.params.iter().enumerate() {
            let suffix = if p.is_optional { "?: string" } else { ": string" };
            let comma = if i + 1 < sig.params.len() { "," } else { "" };
            out.push_str(&format!("  {}{}{}\n", p.name, suffix, comma));
        }
        out.push_str(&format!(") -> {} {{\n", sig.return_type_name));
    }

    out.push_str(&format!("  client {}\n", client));
    out.push_str("  prompt #\"\n");
    out.push_str("    {{ _.role(\"system\") }}\n");
    out.push_str(&format!("    {}\n\n", system_prompt));
    out.push_str("    {{ _.role(\"user\") }}\n");

    // Primary "task" kwarg rendered as the main prompt body
    if sig.params.iter().any(|p| p.name == "task" && !p.is_optional) {
        out.push_str("    Task: {{ task }}\n");
    }

    // Optional kwarg guards
    for p in sig.params.iter().filter(|p| p.is_optional) {
        out.push_str(&format!("    {{% if {} %}}\n", p.name));
        out.push_str(&format!("    {}: {{{{ {} }}}}\n", p.name, p.name));
        out.push_str("    {% endif %}\n");
    }

    out.push_str("  \"#\n");
    out.push_str("}\n");
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{
        AgentDecl, AgentSettings, Block, ClientDecl, Constraint, DataType, Document, Expr,
        Span, SpannedExpr, Statement,
        TypeDecl, TypeField, WorkflowDecl,
    };

    fn span() -> Span {
        0..0
    }

    fn spanned(expr: Expr) -> SpannedExpr {
        SpannedExpr { expr, span: span() }
    }

    fn empty_doc() -> Document {
        Document {
            imports: vec![],
            types: vec![],
            clients: vec![],
            tools: vec![],
            agents: vec![],
            workflows: vec![],
            listeners: vec![],
            tests: vec![],
            mocks: vec![],
            span: span(),
        }
    }

    fn make_agent(name: &str, extends: Option<&str>, client: Option<&str>, system_prompt: Option<&str>, tools: Vec<&str>) -> AgentDecl {
        AgentDecl {
            name: name.to_owned(),
            extends: extends.map(str::to_owned),
            client: client.map(str::to_owned),
            system_prompt: system_prompt.map(str::to_owned),
            tools: tools.into_iter().map(str::to_owned).collect(),
            settings: AgentSettings { entries: vec![], span: span() },
            span: span(),
        }
    }

    #[test]
    fn test_resolve_agent_inherits_client() {
        let doc = Document {
            agents: vec![
                make_agent("Base", None, Some("MyClient"), None, vec![]),
                make_agent("Child", Some("Base"), None, None, vec![]),
            ],
            ..empty_doc()
        };
        let resolved = resolve_agents(&doc).unwrap();
        let child = resolved.iter().find(|a| a.name == "Child").unwrap();
        assert_eq!(child.client.as_deref(), Some("MyClient"));
    }

    #[test]
    fn test_resolve_agent_inherits_system_prompt() {
        let doc = Document {
            agents: vec![
                make_agent("Base", None, None, Some("Be deterministic."), vec![]),
                make_agent("SeniorResearcher", Some("Base"), None, None, vec![]),
            ],
            ..empty_doc()
        };
        let resolved = resolve_agents(&doc).unwrap();
        let senior = resolved.iter().find(|a| a.name == "SeniorResearcher").unwrap();
        assert_eq!(senior.system_prompt.as_deref(), Some("Be deterministic."));
    }

    #[test]
    fn test_collect_call_sites_single_agent_single_type() {
        let doc = Document {
            workflows: vec![WorkflowDecl {
                name: "W".to_owned(),
                arguments: vec![],
                return_type: None,
                body: Block {
                    statements: vec![Statement::ExecuteRun {
                        agent_name: "Researcher".to_owned(),
                        kwargs: vec![("task".to_owned(), spanned(Expr::StringLiteral("hi".to_owned())))],
                        require_type: Some(DataType::Custom("SearchResult".to_owned(), span())),
                        span: span(),
                    }],
                    span: span(),
                },
                span: span(),
            }],
            ..empty_doc()
        };
        let sites = collect_call_sites(&doc);
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].baml_function_name, "ResearcherRun_SearchResult");
        assert_eq!(sites[0].params.len(), 1);
        assert_eq!(sites[0].params[0].name, "task");
        assert!(!sites[0].params[0].is_optional);
    }

    #[test]
    fn test_collect_call_sites_single_agent_two_types() {
        let doc = Document {
            workflows: vec![WorkflowDecl {
                name: "W".to_owned(),
                arguments: vec![],
                return_type: None,
                body: Block {
                    statements: vec![
                        Statement::ExecuteRun {
                            agent_name: "Researcher".to_owned(),
                            kwargs: vec![("task".to_owned(), spanned(Expr::StringLiteral("a".to_owned())))],
                            require_type: Some(DataType::Custom("SearchResult".to_owned(), span())),
                            span: span(),
                        },
                        Statement::ExecuteRun {
                            agent_name: "Researcher".to_owned(),
                            kwargs: vec![("task".to_owned(), spanned(Expr::StringLiteral("b".to_owned())))],
                            require_type: Some(DataType::Custom("VerifiedUser".to_owned(), span())),
                            span: span(),
                        },
                    ],
                    span: span(),
                },
                span: span(),
            }],
            ..empty_doc()
        };
        let sites = collect_call_sites(&doc);
        assert_eq!(sites.len(), 2);
        let names: Vec<&str> = sites.iter().map(|s| s.baml_function_name.as_str()).collect();
        assert!(names.contains(&"ResearcherRun_SearchResult"));
        assert!(names.contains(&"ResearcherRun_VerifiedUser"));
    }

    #[test]
    fn test_collect_call_sites_optional_params() {
        let doc = Document {
            workflows: vec![WorkflowDecl {
                name: "W".to_owned(),
                arguments: vec![],
                return_type: None,
                body: Block {
                    statements: vec![
                        Statement::ExecuteRun {
                            agent_name: "Researcher".to_owned(),
                            kwargs: vec![("task".to_owned(), spanned(Expr::StringLiteral("a".to_owned())))],
                            require_type: Some(DataType::Custom("SearchResult".to_owned(), span())),
                            span: span(),
                        },
                        Statement::ExecuteRun {
                            agent_name: "Researcher".to_owned(),
                            kwargs: vec![
                                ("task".to_owned(), spanned(Expr::StringLiteral("b".to_owned()))),
                                ("context".to_owned(), spanned(Expr::StringLiteral("c".to_owned()))),
                            ],
                            require_type: Some(DataType::Custom("SearchResult".to_owned(), span())),
                            span: span(),
                        },
                    ],
                    span: span(),
                },
                span: span(),
            }],
            ..empty_doc()
        };
        let sites = collect_call_sites(&doc);
        assert_eq!(sites.len(), 1);
        let task_param = sites[0].params.iter().find(|p| p.name == "task").unwrap();
        let context_param = sites[0].params.iter().find(|p| p.name == "context").unwrap();
        assert!(!task_param.is_optional);
        assert!(context_param.is_optional);
    }

    #[test]
    fn test_skip_tool_using_agent() {
        let resolved = vec![ResolvedAgent {
            name: "ToolAgent".to_owned(),
            client: Some("MyClient".to_owned()),
            system_prompt: None,
            tools: vec!["Browser.search".to_owned()],
            span: span(),
        }];
        let sites = vec![CallSiteSignature {
            agent_name: "ToolAgent".to_owned(),
            return_type_name: "SearchResult".to_owned(),
            params: vec![],
            baml_function_name: "ToolAgentRun_SearchResult".to_owned(),
        }];
        let functions = emit_functions(&resolved, &sites);
        assert!(functions.is_empty(), "Tool-using agent must not generate BAML functions");
    }

    #[test]
    fn test_emit_baml_generator_block() {
        let generators = emit_generators();
        assert!(generators.contains("output_type \"typescript\""));
        assert!(generators.contains("version \"0.70.0\""));
        assert!(generators.contains("output_dir \"../baml_client\""));
    }

    #[test]
    fn test_emit_baml_client_openai() {
        let clients = vec![ClientDecl {
            name: "FastOpenAI".to_owned(),
            provider: "openai".to_owned(),
            model: "gpt-4o".to_owned(),
            retries: Some(3),
            timeout_ms: None,
            endpoint: None,
            api_key: None,
            span: span(),
        }];
        let output = emit_clients(&clients);
        assert!(output.contains("client<llm> FastOpenAI {"));
        assert!(output.contains("provider openai"));
        assert!(output.contains("model \"gpt-4o\""));
        assert!(output.contains("max_retries 3"));
    }

    #[test]
    fn test_emit_baml_type_with_constraints() {
        let types = vec![TypeDecl {
            name: "SearchResult".to_owned(),
            fields: vec![TypeField {
                name: "confidence_score".to_owned(),
                data_type: DataType::Float(span()),
                constraints: vec![
                    Constraint {
                        name: "min".to_owned(),
                        value: spanned(Expr::FloatLiteral(0.0)),
                        span: span(),
                    },
                    Constraint {
                        name: "max".to_owned(),
                        value: spanned(Expr::FloatLiteral(1.0)),
                        span: span(),
                    },
                ],
                span: span(),
            }],
            span: span(),
        }];
        let output = emit_types(&types).unwrap();
        assert!(output.contains("class SearchResult {"));
        assert!(output.contains("confidence_score float"));
        assert!(output.contains("@check(min,"));
        assert!(output.contains("@check(max,"));
    }

    #[test]
    fn test_emit_baml_function_with_optional_param() {
        let sig = CallSiteSignature {
            agent_name: "Researcher".to_owned(),
            return_type_name: "SearchResult".to_owned(),
            params: vec![
                CallSiteParam { name: "task".to_owned(), is_optional: false },
                CallSiteParam { name: "context".to_owned(), is_optional: true },
            ],
            baml_function_name: "ResearcherRun_SearchResult".to_owned(),
        };
        let mut out = String::new();
        emit_function(&mut out, &sig, "FastOpenAI", "Stay deterministic.");
        assert!(out.contains("function ResearcherRun_SearchResult("));
        assert!(out.contains("task: string"));
        assert!(out.contains("context?: string"));
        assert!(out.contains("{% if context %}"));
        assert!(out.contains("{% endif %}"));
        assert!(out.contains("-> SearchResult {"));
    }
}
