use crate::ast::{Constraint, DataType, Document, Expr, TypeDecl, TypeField, WorkflowDecl};
use crate::codegen::document_ast_hash;
use crate::errors::{CompilerError, CompilerResult};

pub(super) fn generate(document: &Document) -> CompilerResult<String> {
    let document_hash = document_ast_hash(document);
    let mut sections = vec![[
        "from typing import List",
        "",
        "from claw_sdk import ClawClient",
        "from pydantic import BaseModel, Field",
    ]
    .join("\n")];
    sections.push(format!(r#"CLAW_AST_HASH = "{document_hash}""#));

    for declaration in &document.types {
        sections.push(render_model(declaration)?);
    }

    for workflow in &document.workflows {
        sections.push(render_workflow(workflow));
    }

    Ok(sections.join("\n\n"))
}

fn render_model(declaration: &TypeDecl) -> CompilerResult<String> {
    let fields = declaration
        .fields
        .iter()
        .map(render_model_field)
        .collect::<CompilerResult<Vec<_>>>()?
        .join("\n");

    Ok(format!("class {}(BaseModel):\n{}", declaration.name, fields))
}

fn render_model_field(field: &TypeField) -> CompilerResult<String> {
    let data_type = render_python_type(&field.data_type);
    let field_config = render_field_config(&field.data_type, &field.constraints)?;

    Ok(match field_config {
        Some(config) => format!("    {}: {} = Field({})", field.name, data_type, config),
        None => format!("    {}: {}", field.name, data_type),
    })
}

fn render_workflow(workflow: &WorkflowDecl) -> String {
    let arguments = workflow
        .arguments
        .iter()
        .map(|argument| format!("    {}: {},", argument.name, render_python_type(&argument.data_type)))
        .collect::<Vec<_>>()
        .join("\n");

    let argument_lines = if arguments.is_empty() {
        String::new()
    } else {
        format!("{arguments}\n")
    };

    let gateway_arguments = if workflow.arguments.is_empty() {
        "{}".to_owned()
    } else {
        let pairs = workflow
            .arguments
            .iter()
            .map(|argument| format!(r#""{}": {}"#, argument.name, argument.name))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{{{pairs}}}")
    };

    let return_type = workflow
        .return_type
        .as_ref()
        .map(render_python_type)
        .unwrap_or_else(|| "None".to_owned());

    let return_statement = workflow
        .return_type
        .as_ref()
        .map(render_return_statement)
        .unwrap_or_else(|| "    return None".to_owned());

    format!(
        "async def {}(\n{}    client: ClawClient,\n    resume_session_id: str | None = None,\n) -> {}:\n    result_dict = await client.execute_workflow(\n        workflow_name=\"{}\",\n        arguments={},\n        ast_hash=CLAW_AST_HASH,\n        resume_session_id=resume_session_id,\n    )\n\n{}",
        to_snake_case(&workflow.name),
        argument_lines,
        return_type,
        workflow.name,
        gateway_arguments,
        return_statement
    )
}

fn render_return_statement(data_type: &DataType) -> String {
    match data_type {
        DataType::Custom(name, _) => format!("    return {name}(**result_dict)"),
        _ => "    return result_dict".to_owned(),
    }
}

fn render_python_type(data_type: &DataType) -> String {
    match data_type {
        DataType::String(_) => "str".to_owned(),
        DataType::Int(_) => "int".to_owned(),
        DataType::Float(_) => "float".to_owned(),
        DataType::Boolean(_) => "bool".to_owned(),
        DataType::List(inner, _) => format!("List[{}]", render_python_type(inner)),
        DataType::Custom(name, _) => name.clone(),
    }
}

fn render_field_config(
    data_type: &DataType,
    constraints: &[Constraint],
) -> CompilerResult<Option<String>> {
    if constraints.is_empty() {
        return Ok(None);
    }

    let mut parts = Vec::new();

    for constraint in constraints {
        match constraint.name.as_str() {
            "min" => parts.push(render_min_constraint(data_type, constraint)?),
            "max" => parts.push(render_max_constraint(data_type, constraint)?),
            "regex" => parts.push(render_regex_constraint(constraint)?),
            other => {
                return Err(CompilerError::UnsupportedConstraint {
                    name: other.to_owned(),
                    span: constraint.span.clone(),
                })
            }
        }
    }

    Ok(Some(parts.join(", ")))
}

fn render_min_constraint(data_type: &DataType, constraint: &Constraint) -> CompilerResult<String> {
    let value = render_numeric_constraint_value(constraint)?;
    Ok(match data_type {
        DataType::String(_) => format!("min_length={value}"),
        DataType::List(_, _) => format!("min_items={value}"),
        _ => format!("ge={value}"),
    })
}

fn render_max_constraint(data_type: &DataType, constraint: &Constraint) -> CompilerResult<String> {
    let value = render_numeric_constraint_value(constraint)?;
    Ok(match data_type {
        DataType::String(_) => format!("max_length={value}"),
        DataType::List(_, _) => format!("max_items={value}"),
        _ => format!("le={value}"),
    })
}

fn render_regex_constraint(constraint: &Constraint) -> CompilerResult<String> {
    match &constraint.value.expr {
        Expr::StringLiteral(pattern) => Ok(format!(r#"pattern=r"{}""#, escape_python_pattern(pattern))),
        _ => Err(CompilerError::InvalidConstraintValue {
            name: constraint.name.clone(),
            expected: "a string literal".to_owned(),
            span: constraint.span.clone(),
        }),
    }
}

fn render_numeric_constraint_value(constraint: &Constraint) -> CompilerResult<String> {
    match &constraint.value.expr {
        Expr::IntLiteral(value) => Ok(value.to_string()),
        Expr::FloatLiteral(value) => Ok(trim_float(*value)),
        _ => Err(CompilerError::InvalidConstraintValue {
            name: constraint.name.clone(),
            expected: "a numeric literal".to_owned(),
            span: constraint.span.clone(),
        }),
    }
}

fn escape_python_pattern(pattern: &str) -> String {
    pattern.replace('"', "\\\"")
}

fn trim_float(value: f64) -> String {
    let mut rendered = value.to_string();
    if rendered.ends_with(".0") {
        rendered.truncate(rendered.len() - 2);
    }
    rendered
}

fn to_snake_case(name: &str) -> String {
    let mut output = String::with_capacity(name.len() + 4);

    for (index, character) in name.chars().enumerate() {
        if character.is_ascii_uppercase() {
            if index > 0 {
                output.push('_');
            }
            output.push(character.to_ascii_lowercase());
        } else {
            output.push(character);
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::generate;
    use crate::ast::{
        Block, ClientDecl, Constraint, DataType, Document, Expr, Span, SpannedExpr, Statement,
        ToolDecl, TypeDecl, TypeField, WorkflowDecl,
    };

    fn spanned(expr: Expr, span: Span) -> SpannedExpr {
        SpannedExpr { expr, span }
    }

    fn normalize_ast_hash(output: &str) -> String {
        let prefix = r#"CLAW_AST_HASH = ""#;
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
    fn emits_python_sdk_snapshot_for_valid_document() {
        let output = generate(&valid_document()).unwrap();

        insta::assert_snapshot!(normalize_ast_hash(&output), @r#"
        from typing import List

        from claw_sdk import ClawClient
        from pydantic import BaseModel, Field

        CLAW_AST_HASH = "<ast_hash>"

        class SearchResult(BaseModel):
            url: str = Field(pattern=r"^https://")
            confidence_score: float = Field(ge=0)
            snippet: str
            tags: List[str]

        async def analyze_competitors(
            company: str,
            client: ClawClient,
            resume_session_id: str | None = None,
        ) -> SearchResult:
            result_dict = await client.execute_workflow(
                workflow_name="AnalyzeCompetitors",
                arguments={"company": company},
                ast_hash=CLAW_AST_HASH,
                resume_session_id=resume_session_id,
            )

            return SearchResult(**result_dict)
        "#);
    }

    #[test]
    fn lowers_python_field_constraints_into_pydantic_fields() {
        let output = generate(&valid_document()).unwrap();

        assert!(output.contains(r#"Field(pattern=r"^https://")"#));
        assert!(output.contains("Field(ge=0)"));
    }

    fn valid_document() -> Document {
        Document {
            imports: Vec::new(),
            types: vec![TypeDecl {
                name: "SearchResult".to_owned(),
                fields: vec![
                    TypeField {
                        name: "url".to_owned(),
                        data_type: DataType::String(10..16),
                        constraints: vec![Constraint {
                            name: "regex".to_owned(),
                            value: spanned(Expr::StringLiteral("^https://".to_owned()), 17..37),
                            span: 17..37,
                        }],
                        span: 10..37,
                    },
                    TypeField {
                        name: "confidence_score".to_owned(),
                        data_type: DataType::Float(38..43),
                        constraints: vec![Constraint {
                            name: "min".to_owned(),
                            value: spanned(Expr::IntLiteral(0), 44..51),
                            span: 44..51,
                        }],
                        span: 38..51,
                    },
                    TypeField {
                        name: "snippet".to_owned(),
                        data_type: DataType::String(52..58),
                        constraints: Vec::new(),
                        span: 52..66,
                    },
                    TypeField {
                        name: "tags".to_owned(),
                        data_type: DataType::List(Box::new(DataType::String(72..78)), 67..79),
                        constraints: Vec::new(),
                        span: 67..79,
                    },
                ],
                span: 0..79,
            }],
            clients: vec![ClientDecl {
                name: "FastOpenAI".to_owned(),
                provider: "openai".to_owned(),
                model: "gpt-5.1".to_owned(),
                retries: Some(2),
                timeout_ms: Some(5_000),
                endpoint: None,
                api_key: None,
                span: 80..110,
            }],
            tools: vec![ToolDecl {
                name: "WebSearch".to_owned(),
                arguments: vec![TypeField {
                    name: "query".to_owned(),
                    data_type: DataType::String(111..117),
                    constraints: Vec::new(),
                    span: 111..125,
                }],
                return_type: Some(DataType::Custom("SearchResult".to_owned(), 126..138)),
                invoke_path: Some("module(\"scripts.search\").function(\"run\")".to_owned()),
                span: 111..165,
            }],
            agents: Vec::new(),
            workflows: vec![WorkflowDecl {
                name: "AnalyzeCompetitors".to_owned(),
                arguments: vec![TypeField {
                    name: "company".to_owned(),
                    data_type: DataType::String(166..172),
                    constraints: Vec::new(),
                    span: 166..180,
                }],
                return_type: Some(DataType::Custom("SearchResult".to_owned(), 181..193)),
                body: Block {
                    statements: vec![
                        Statement::LetDecl {
                            name: "report".to_owned(),
                            explicit_type: Some(DataType::Custom(
                                "SearchResult".to_owned(),
                                194..206,
                            )),
                            value: spanned(Expr::Identifier("company".to_owned()), 194..220),
                            span: 194..220,
                        },
                        Statement::Return {
                            value: spanned(Expr::Identifier("report".to_owned()), 221..234),
                            span: 221..234,
                        },
                    ],
                    span: 194..234,
                },
                span: 166..234,
            }],
            listeners: Vec::new(),
            tests: Vec::new(),
            mocks: Vec::new(),
            span: 0..234,
        }
    }
}
