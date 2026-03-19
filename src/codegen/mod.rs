mod python;
mod typescript;
pub mod opencode;
pub mod mcp;
pub mod baml;

use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use crate::ast::{
    AgentDecl, AgentSetting, AgentSettings, BinaryOp, Block, ClientDecl, Constraint, DataType,
    Document, ElseBranch, Expr, ImportDecl, ListenerDecl, MockDecl, SettingValue, SpannedExpr,
    Statement, TestDecl, ToolDecl, TypeDecl, TypeField, WorkflowDecl,
};
use crate::errors::CompilerResult;

pub use baml::{BamlOutput, CallSiteSignature, ResolvedAgent, collect_call_sites, generate_baml, resolve_agents};

pub fn generate_ts(document: &Document) -> CompilerResult<String> {
    typescript::generate(document)
}

pub fn generate_python(document: &Document) -> CompilerResult<String> {
    python::generate(document)
}

pub fn generate_opencode(document: &Document, output_dir: &std::path::Path) -> CompilerResult<()> {
    opencode::generate(document, output_dir)
}

pub fn generate_mcp(document: &Document, output_dir: &std::path::Path) -> CompilerResult<()> {
    mcp::generate(document, output_dir)
}

pub fn document_ast_hash(document: &Document) -> String {
    let mut canonical = String::new();
    write_document(&mut canonical, document);

    let digest = Sha256::digest(canonical.as_bytes());
    digest.iter().fold(String::with_capacity(64), |mut output, byte| {
        let _ = write!(output, "{byte:02x}");
        output
    })
}

fn write_document(output: &mut String, document: &Document) {
    write_seq(output, "imports", &document.imports, write_import);
    write_seq(output, "types", &document.types, write_type_decl);
    write_seq(output, "clients", &document.clients, write_client_decl);
    write_seq(output, "tools", &document.tools, write_tool_decl);
    write_seq(output, "agents", &document.agents, write_agent_decl);
    write_seq(output, "workflows", &document.workflows, write_workflow_decl);
    write_seq(output, "listeners", &document.listeners, write_listener_decl);
    write_seq(output, "tests", &document.tests, write_test_decl);
    write_seq(output, "mocks", &document.mocks, write_mock_decl);
}

fn write_import(output: &mut String, declaration: &ImportDecl) {
    write_seq(output, "names", &declaration.names, |output, value| {
        write_string(output, value)
    });
    write_tag(output, "source");
    write_string(output, &declaration.source);
}

fn write_type_decl(output: &mut String, declaration: &TypeDecl) {
    write_tag(output, "name");
    write_string(output, &declaration.name);
    write_seq(output, "fields", &declaration.fields, write_type_field);
}

fn write_type_field(output: &mut String, field: &TypeField) {
    write_tag(output, "name");
    write_string(output, &field.name);
    write_tag(output, "type");
    write_data_type(output, &field.data_type);
    write_seq(output, "constraints", &field.constraints, write_constraint);
}

fn write_constraint(output: &mut String, constraint: &Constraint) {
    write_tag(output, "name");
    write_string(output, &constraint.name);
    write_tag(output, "value");
    write_spanned_expr(output, &constraint.value);
}

fn write_client_decl(output: &mut String, declaration: &ClientDecl) {
    write_tag(output, "name");
    write_string(output, &declaration.name);
    write_tag(output, "provider");
    write_string(output, &declaration.provider);
    write_tag(output, "model");
    write_string(output, &declaration.model);
    write_option_u32(output, "retries", declaration.retries);
    write_option_u32(output, "timeout_ms", declaration.timeout_ms);
    write_option_expr(output, "endpoint", declaration.endpoint.as_ref());
    write_option_expr(output, "api_key", declaration.api_key.as_ref());
}

fn write_tool_decl(output: &mut String, declaration: &ToolDecl) {
    write_tag(output, "name");
    write_string(output, &declaration.name);
    write_seq(output, "arguments", &declaration.arguments, write_type_field);
    write_option_data_type(output, "return_type", declaration.return_type.as_ref());
    write_option_string(output, "invoke_path", declaration.invoke_path.as_deref());
}

fn write_agent_decl(output: &mut String, declaration: &AgentDecl) {
    write_tag(output, "name");
    write_string(output, &declaration.name);
    write_option_string(output, "extends", declaration.extends.as_deref());
    write_option_string(output, "client", declaration.client.as_deref());
    write_option_string(
        output,
        "system_prompt",
        declaration.system_prompt.as_deref(),
    );
    write_seq(output, "tools", &declaration.tools, |output, value| {
        write_string(output, value)
    });
    write_agent_settings(output, &declaration.settings);
}

fn write_agent_settings(output: &mut String, settings: &AgentSettings) {
    write_seq(output, "settings", &settings.entries, write_agent_setting);
}

fn write_agent_setting(output: &mut String, setting: &AgentSetting) {
    write_tag(output, "name");
    write_string(output, &setting.name);
    write_tag(output, "value");
    write_setting_value(output, &setting.value);
}

fn write_workflow_decl(output: &mut String, declaration: &WorkflowDecl) {
    write_tag(output, "name");
    write_string(output, &declaration.name);
    write_seq(output, "arguments", &declaration.arguments, write_type_field);
    write_option_data_type(output, "return_type", declaration.return_type.as_ref());
    write_tag(output, "body");
    write_block(output, &declaration.body);
}

fn write_listener_decl(output: &mut String, declaration: &ListenerDecl) {
    write_tag(output, "name");
    write_string(output, &declaration.name);
    write_tag(output, "event_type");
    write_string(output, &declaration.event_type);
    write_tag(output, "body");
    write_block(output, &declaration.body);
}

fn write_test_decl(output: &mut String, declaration: &TestDecl) {
    write_tag(output, "name");
    write_string(output, &declaration.name);
    write_tag(output, "body");
    write_block(output, &declaration.body);
}

fn write_mock_decl(output: &mut String, declaration: &MockDecl) {
    write_tag(output, "target_agent");
    write_string(output, &declaration.target_agent);
    write_seq(output, "output", &declaration.output, |output, pair| {
        write_tag(output, "key");
        write_string(output, &pair.0);
        write_tag(output, "value");
        write_spanned_expr(output, &pair.1);
    });
}

fn write_block(output: &mut String, block: &Block) {
    write_seq(output, "statements", &block.statements, write_statement);
}

fn write_statement(output: &mut String, statement: &Statement) {
    match statement {
        Statement::LetDecl {
            name,
            explicit_type,
            value,
            ..
        } => {
            write_tag(output, "let");
            write_string(output, name);
            write_option_data_type(output, "explicit_type", explicit_type.as_ref());
            write_tag(output, "value");
            write_spanned_expr(output, value);
        }
        Statement::ForLoop {
            item_name,
            iterator,
            body,
            ..
        } => {
            write_tag(output, "for");
            write_string(output, item_name);
            write_spanned_expr(output, iterator);
            write_block(output, body);
        }
        Statement::IfCond {
            condition,
            if_body,
            else_body,
            ..
        } => {
            write_tag(output, "if");
            write_spanned_expr(output, condition);
            write_block(output, if_body);
            match else_body {
                Some(ElseBranch::Else(block)) => {
                    write_tag(output, "else_some");
                    write_block(output, block);
                }
                Some(ElseBranch::ElseIf(stmt)) => {
                    write_tag(output, "else_if");
                    write_statement(output, stmt);
                }
                None => write_tag(output, "else_none"),
            }
        }
        Statement::ExecuteRun {
            agent_name,
            kwargs,
            require_type,
            ..
        } => {
            write_tag(output, "execute_stmt");
            write_string(output, agent_name);
            write_seq(output, "kwargs", kwargs, write_kwarg);
            write_option_data_type(output, "require_type", require_type.as_ref());
        }
        Statement::Return { value, .. } => {
            write_tag(output, "return");
            write_spanned_expr(output, value);
        }
        Statement::Expression(spanned) => {
            write_tag(output, "expression");
            write_spanned_expr(output, spanned);
        }
        Statement::TryCatch {
            try_body,
            catch_name,
            catch_type,
            catch_body,
            ..
        } => {
            write_tag(output, "try_catch");
            write_block(output, try_body);
            write_string(output, catch_name);
            write_data_type(output, catch_type);
            write_block(output, catch_body);
        }
        Statement::Assert {
            condition,
            message,
            ..
        } => {
            write_tag(output, "assert");
            write_spanned_expr(output, condition);
            write_option_string(output, "message", message.as_deref());
        }
        Statement::Continue(_) => write_tag(output, "continue"),
        Statement::Break(_) => write_tag(output, "break"),
    }
}

fn write_spanned_expr(output: &mut String, spanned: &SpannedExpr) {
    write_expr(output, &spanned.expr);
}

fn write_expr(output: &mut String, expr: &Expr) {
    match expr {
        Expr::StringLiteral(value) => {
            write_tag(output, "string");
            write_string(output, value);
        }
        Expr::IntLiteral(value) => {
            write_tag(output, "int");
            let _ = write!(output, "{value};");
        }
        Expr::FloatLiteral(value) => {
            write_tag(output, "float");
            write_string(output, &trim_float(*value));
        }
        Expr::BoolLiteral(value) => {
            write_tag(output, "bool");
            let _ = write!(output, "{value};");
        }
        Expr::Identifier(value) => {
            write_tag(output, "identifier");
            write_string(output, value);
        }
        Expr::ArrayLiteral(values) => write_seq(output, "array", values, write_spanned_expr),
        Expr::Call(name, args) => {
            write_tag(output, "call");
            write_string(output, name);
            write_seq(output, "args", args, write_spanned_expr);
        }
        Expr::MemberAccess(target, member) => {
            write_tag(output, "member_access");
            write_spanned_expr(output, target);
            write_string(output, member);
        }
        Expr::MethodCall(target, method, args) => {
            write_tag(output, "method_call");
            write_spanned_expr(output, target);
            write_string(output, method);
            write_seq(output, "args", args, write_spanned_expr);
        }
        Expr::ExecuteRun {
            agent_name,
            kwargs,
            require_type,
        } => {
            write_tag(output, "execute_expr");
            write_string(output, agent_name);
            write_seq(output, "kwargs", kwargs, write_kwarg);
            write_option_data_type(output, "require_type", require_type.as_ref());
        }
        Expr::BinaryOp { left, op, right } => {
            write_tag(output, "binary");
            write_spanned_expr(output, left);
            match op {
                BinaryOp::Equal => write_tag(output, "equal"),
                BinaryOp::NotEqual => write_tag(output, "not_equal"),
                BinaryOp::LessThan => write_tag(output, "less_than"),
                BinaryOp::GreaterThan => write_tag(output, "greater_than"),
                BinaryOp::LessEq => write_tag(output, "less_eq"),
                BinaryOp::GreaterEq => write_tag(output, "greater_eq"),
            }
            write_spanned_expr(output, right);
        }
    }
}

fn write_kwarg(output: &mut String, argument: &(String, SpannedExpr)) {
    write_tag(output, "kwarg");
    write_string(output, &argument.0);
    write_spanned_expr(output, &argument.1);
}

fn write_setting_value(output: &mut String, value: &SettingValue) {
    match value {
        SettingValue::Int(value) => {
            write_tag(output, "setting_int");
            let _ = write!(output, "{value};");
        }
        SettingValue::Float(value) => {
            write_tag(output, "setting_float");
            write_string(output, &trim_float(*value));
        }
        SettingValue::Boolean(value) => {
            write_tag(output, "setting_bool");
            let _ = write!(output, "{value};");
        }
    }
}

fn write_data_type(output: &mut String, data_type: &DataType) {
    match data_type {
        DataType::String(_) => write_tag(output, "type_string"),
        DataType::Int(_) => write_tag(output, "type_int"),
        DataType::Float(_) => write_tag(output, "type_float"),
        DataType::Boolean(_) => write_tag(output, "type_boolean"),
        DataType::List(inner, _) => {
            write_tag(output, "type_list");
            write_data_type(output, inner);
        }
        DataType::Custom(name, _) => {
            write_tag(output, "type_custom");
            write_string(output, name);
        }
    }
}

fn write_option_data_type(output: &mut String, tag: &str, data_type: Option<&DataType>) {
    match data_type {
        Some(data_type) => {
            write_tag(output, tag);
            write_data_type(output, data_type);
        }
        None => write_tag(output, &format!("{tag}_none")),
    }
}

fn write_option_string(output: &mut String, tag: &str, value: Option<&str>) {
    match value {
        Some(value) => {
            write_tag(output, tag);
            write_string(output, value);
        }
        None => write_tag(output, &format!("{tag}_none")),
    }
}

fn write_option_u32(output: &mut String, tag: &str, value: Option<u32>) {
    match value {
        Some(value) => {
            write_tag(output, tag);
            let _ = write!(output, "{value};");
        }
        None => write_tag(output, &format!("{tag}_none")),
    }
}

fn write_option_expr(output: &mut String, tag: &str, expr: Option<&SpannedExpr>) {
    match expr {
        Some(spanned) => {
            write_tag(output, tag);
            write_spanned_expr(output, spanned);
        }
        None => write_tag(output, &format!("{tag}_none")),
    }
}

fn write_seq<T>(
    output: &mut String,
    tag: &str,
    values: &[T],
    mut write_value: impl FnMut(&mut String, &T),
) {
    write_tag(output, tag);
    let _ = write!(output, "{}[", values.len());
    for value in values {
        write_value(output, value);
    }
    output.push_str("];");
}

fn write_tag(output: &mut String, tag: &str) {
    output.push_str(tag);
    output.push(':');
}

fn write_string(output: &mut String, value: &str) {
    let _ = write!(output, "{}:{};", value.len(), value);
}

fn trim_float(value: f64) -> String {
    let mut rendered = value.to_string();
    if rendered.ends_with(".0") {
        rendered.truncate(rendered.len() - 2);
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::generate_ts;
    use crate::ast::{
        AgentDecl, AgentSetting, AgentSettings, Block, ClientDecl, Constraint, DataType, Document,
        Expr, SettingValue, Span, SpannedExpr, Statement, ToolDecl, TypeDecl, TypeField,
        WorkflowDecl,
    };

    fn spanned(expr: Expr, span: Span) -> SpannedExpr {
        SpannedExpr { expr, span }
    }

    fn normalize_ast_hash(output: &str) -> String {
        let prefix = r#"export const CLAW_AST_HASH = ""#;
        let Some(pos) = output.find(prefix) else {
            return output.to_owned();
        };
        let hash_start = pos + prefix.len();
        let hash_end = hash_start + 64;
        if hash_end > output.len() {
            return output.to_owned();
        }
        format!("{}<ast_hash>{}", &output[..hash_start], &output[hash_end..])
    }

    #[test]
    fn emits_typescript_sdk_snapshot_for_valid_document() {
        let output = generate_ts(&valid_document()).unwrap();

        insta::assert_snapshot!(normalize_ast_hash(&output), @r#"
        import { z } from "zod";
        import { ClawClient } from "@claw/sdk";

        export const CLAW_AST_HASH = "<ast_hash>";

        export interface SearchResult {
            url: string;
            confidence_score: number;
            snippet: string;
            tags: string[];
        }

        export const SearchResultSchema: z.ZodType<SearchResult> = z.object({
            url: z.string(),
            confidence_score: z.number().min(0),
            snippet: z.string(),
            tags: z.array(z.string()),
        }).strict();

        export interface VerifiedUser {
            email: string;
            age: number;
        }

        export const VerifiedUserSchema: z.ZodType<VerifiedUser> = z.object({
            email: z.string().regex(new RegExp("^[\\w-\\.]+@([\\w-]+\\.)+[\\w-]{2,4}$")),
            age: z.number().int().min(18),
        }).strict();

        export const AnalyzeCompetitors = async (
            company: string,
            options: { client: ClawClient; resumeSessionId?: string }
        ): Promise<SearchResult> => {
            const result = await options.client.executeWorkflow({
                workflowName: "AnalyzeCompetitors",
                arguments: { company },
                astHash: CLAW_AST_HASH,
                resumeSessionId: options.resumeSessionId,
            });

            return SearchResultSchema.parse(result);
        };

        export const VerifyUser = async (
            email: string,
            options: { client: ClawClient; resumeSessionId?: string }
        ): Promise<VerifiedUser> => {
            const result = await options.client.executeWorkflow({
                workflowName: "VerifyUser",
                arguments: { email },
                astHash: CLAW_AST_HASH,
                resumeSessionId: options.resumeSessionId,
            });

            return VerifiedUserSchema.parse(result);
        };
        "#);
    }

    #[test]
    fn lowers_field_constraints_into_zod_validators() {
        let output = generate_ts(&valid_document()).unwrap();

        assert!(output.contains(r#"z.string().regex(new RegExp("^[\\w-\\.]+@([\\w-]+\\.)+[\\w-]{2,4}$"))"#));
        assert!(output.contains("z.number().int().min(18)"));
        assert!(output.contains("z.number().min(0)"));
    }

    #[test]
    fn emits_workflow_wrappers_with_gateway_calls() {
        let output = generate_ts(&valid_document()).unwrap();

        assert!(output.contains("export const AnalyzeCompetitors = async ("));
        assert!(output.contains(r#"workflowName: "AnalyzeCompetitors""#));
        assert!(output.contains("arguments: { company }"));
        assert!(output.contains("astHash: CLAW_AST_HASH"));
        assert!(output.contains("return SearchResultSchema.parse(result);"));
    }

    fn valid_document() -> Document {
        Document {
            imports: Vec::new(),
            types: vec![
                TypeDecl {
                    name: "SearchResult".to_owned(),
                    fields: vec![
                        TypeField {
                            name: "url".to_owned(),
                            data_type: DataType::String(10..16),
                            constraints: Vec::new(),
                            span: 10..23,
                        },
                        TypeField {
                            name: "confidence_score".to_owned(),
                            data_type: DataType::Float(24..29),
                            constraints: vec![Constraint {
                                name: "min".to_owned(),
                                value: spanned(Expr::IntLiteral(0), 30..37),
                                span: 30..37,
                            }],
                            span: 24..37,
                        },
                        TypeField {
                            name: "snippet".to_owned(),
                            data_type: DataType::String(38..44),
                            constraints: Vec::new(),
                            span: 38..52,
                        },
                        TypeField {
                            name: "tags".to_owned(),
                            data_type: DataType::List(Box::new(DataType::String(58..64)), 53..65),
                            constraints: Vec::new(),
                            span: 53..65,
                        },
                    ],
                    span: 0..65,
                },
                TypeDecl {
                    name: "VerifiedUser".to_owned(),
                    fields: vec![
                        TypeField {
                            name: "email".to_owned(),
                            data_type: DataType::String(70..76),
                            constraints: vec![Constraint {
                                name: "regex".to_owned(),
                                value: spanned(
                                    Expr::StringLiteral(
                                        "^[\\w-\\.]+@([\\w-]+\\.)+[\\w-]{2,4}$".to_owned(),
                                    ),
                                    77..125,
                                ),
                                span: 77..125,
                            }],
                            span: 70..125,
                        },
                        TypeField {
                            name: "age".to_owned(),
                            data_type: DataType::Int(126..129),
                            constraints: vec![Constraint {
                                name: "min".to_owned(),
                                value: spanned(Expr::IntLiteral(18), 130..138),
                                span: 130..138,
                            }],
                            span: 126..138,
                        },
                    ],
                    span: 66..138,
                },
            ],
            clients: vec![ClientDecl {
                name: "FastOpenAI".to_owned(),
                provider: "openai".to_owned(),
                model: "gpt-5.1".to_owned(),
                retries: Some(2),
                timeout_ms: Some(5_000),
                endpoint: None,
                api_key: None,
                span: 139..165,
            }],
            tools: vec![ToolDecl {
                name: "WebSearch".to_owned(),
                arguments: vec![TypeField {
                    name: "query".to_owned(),
                    data_type: DataType::String(166..172),
                    constraints: Vec::new(),
                    span: 166..180,
                }],
                return_type: Some(DataType::Custom("SearchResult".to_owned(), 181..193)),
                invoke_path: Some("module(\"scripts.search\").function(\"run\")".to_owned()),
                span: 166..220,
            }],
            agents: vec![AgentDecl {
                name: "Researcher".to_owned(),
                extends: None,
                client: Some("FastOpenAI".to_owned()),
                system_prompt: Some("Stay deterministic.".to_owned()),
                tools: vec!["WebSearch".to_owned()],
                settings: AgentSettings {
                    entries: vec![
                        AgentSetting {
                            name: "max_steps".to_owned(),
                            value: SettingValue::Int(5),
                            span: 221..233,
                        },
                        AgentSetting {
                            name: "temperature".to_owned(),
                            value: SettingValue::Float(0.1),
                            span: 234..250,
                        },
                    ],
                    span: 221..250,
                },
                span: 221..280,
            }],
            workflows: vec![
                WorkflowDecl {
                    name: "AnalyzeCompetitors".to_owned(),
                    arguments: vec![TypeField {
                        name: "company".to_owned(),
                        data_type: DataType::String(281..287),
                        constraints: Vec::new(),
                        span: 281..295,
                    }],
                    return_type: Some(DataType::Custom("SearchResult".to_owned(), 296..308)),
                    body: Block {
                        statements: vec![
                            Statement::LetDecl {
                                name: "report".to_owned(),
                                explicit_type: Some(DataType::Custom(
                                    "SearchResult".to_owned(),
                                    309..321,
                                )),
                                value: spanned(
                                    Expr::ExecuteRun {
                                        agent_name: "Researcher".to_owned(),
                                        kwargs: vec![(
                                            "task".to_owned(),
                                            spanned(
                                                Expr::Identifier("company".to_owned()),
                                                335..342,
                                            ),
                                        )],
                                        require_type: Some(DataType::Custom(
                                            "SearchResult".to_owned(),
                                            322..334,
                                        )),
                                    },
                                    309..360,
                                ),
                                span: 309..360,
                            },
                            Statement::Return {
                                value: spanned(
                                    Expr::Identifier("report".to_owned()),
                                    361..374,
                                ),
                                span: 361..374,
                            },
                        ],
                        span: 309..374,
                    },
                    span: 281..374,
                },
                WorkflowDecl {
                    name: "VerifyUser".to_owned(),
                    arguments: vec![TypeField {
                        name: "email".to_owned(),
                        data_type: DataType::String(375..381),
                        constraints: Vec::new(),
                        span: 375..387,
                    }],
                    return_type: Some(DataType::Custom("VerifiedUser".to_owned(), 388..400)),
                    body: Block {
                        statements: vec![
                            Statement::LetDecl {
                                name: "user".to_owned(),
                                explicit_type: Some(DataType::Custom(
                                    "VerifiedUser".to_owned(),
                                    401..413,
                                )),
                                value: spanned(
                                    Expr::ExecuteRun {
                                        agent_name: "Researcher".to_owned(),
                                        kwargs: vec![(
                                            "task".to_owned(),
                                            spanned(
                                                Expr::Identifier("email".to_owned()),
                                                427..432,
                                            ),
                                        )],
                                        require_type: Some(DataType::Custom(
                                            "VerifiedUser".to_owned(),
                                            414..426,
                                        )),
                                    },
                                    401..452,
                                ),
                                span: 401..452,
                            },
                            Statement::Return {
                                value: spanned(
                                    Expr::Identifier("user".to_owned()),
                                    453..464,
                                ),
                                span: 453..464,
                            },
                        ],
                        span: 401..464,
                    },
                    span: 375..464,
                },
            ],
            listeners: Vec::new(),
            tests: Vec::new(),
            mocks: Vec::new(),
            span: 0..464,
        }
    }
}
