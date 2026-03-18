use crate::ast::{Constraint, DataType, Document, Expr, TypeDecl, TypeField, WorkflowDecl};
use crate::codegen::document_ast_hash;
use crate::errors::{CompilerError, CompilerResult};

pub(super) fn generate(document: &Document) -> CompilerResult<String> {
    let document_hash = document_ast_hash(document);
    let mut sections = vec![[
        r#"import { z } from "zod";"#,
        r#"import { OpenClawClient } from "@openclaw/sdk";"#,
    ]
    .join("\n")];
    sections.push(format!(
        r#"export const OPENCLAW_AST_HASH = "{document_hash}";"#
    ));

    for declaration in &document.types {
        sections.push(render_interface(declaration));
        sections.push(render_schema(declaration)?);
    }

    for workflow in &document.workflows {
        sections.push(render_workflow(workflow));
    }

    Ok(sections.join("\n\n"))
}

fn render_interface(declaration: &TypeDecl) -> String {
    let fields = declaration
        .fields
        .iter()
        .map(|field| format!("    {}: {};", field.name, render_ts_type(&field.data_type)))
        .collect::<Vec<_>>()
        .join("\n");

    format!("export interface {} {{\n{}\n}}", declaration.name, fields)
}

fn render_schema(declaration: &TypeDecl) -> CompilerResult<String> {
    let fields = declaration
        .fields
        .iter()
        .map(render_schema_field)
        .collect::<CompilerResult<Vec<_>>>()?
        .join("\n");

    Ok(format!(
        "export const {}Schema: z.ZodType<{}> = z.object({{\n{}\n}}).strict();",
        declaration.name, declaration.name, fields
    ))
}

fn render_schema_field(field: &TypeField) -> CompilerResult<String> {
    let schema = apply_constraints(render_zod_type(&field.data_type), &field.constraints)?;
    Ok(format!("    {}: {},", field.name, schema))
}

fn render_workflow(workflow: &WorkflowDecl) -> String {
    let arguments = workflow
        .arguments
        .iter()
        .map(|argument| format!("{}: {}", argument.name, render_ts_type(&argument.data_type)))
        .collect::<Vec<_>>()
        .join(",\n    ");

    let arguments_signature = if arguments.is_empty() {
        String::new()
    } else {
        format!("{arguments},\n    ")
    };

    let gateway_arguments = if workflow.arguments.is_empty() {
        "{}".to_owned()
    } else {
        let names = workflow
            .arguments
            .iter()
            .map(|argument| argument.name.clone())
            .collect::<Vec<_>>()
            .join(", ");
        format!("{{ {names} }}")
    };

    let return_type = workflow
        .return_type
        .as_ref()
        .map(render_ts_type)
        .unwrap_or_else(|| "void".to_owned());

    let return_statement = workflow
        .return_type
        .as_ref()
        .map(render_result_parser)
        .unwrap_or_else(|| "    return;\n".to_owned());

    format!(
        "export const {} = async (\n    {}options: {{ client: OpenClawClient; resumeSessionId?: string }}\n): Promise<{}> => {{\n    const result = await options.client.executeWorkflow({{\n        workflowName: \"{}\",\n        arguments: {},\n        astHash: OPENCLAW_AST_HASH,\n        resumeSessionId: options.resumeSessionId,\n    }});\n\n{}}};",
        workflow.name,
        arguments_signature,
        return_type,
        workflow.name,
        gateway_arguments,
        return_statement
    )
}

fn render_result_parser(data_type: &DataType) -> String {
    let parser = match data_type {
        DataType::Custom(name, _) => format!("{name}Schema"),
        _ => render_zod_type(data_type),
    };

    format!("    return {parser}.parse(result);\n")
}

fn render_ts_type(data_type: &DataType) -> String {
    match data_type {
        DataType::String(_) => "string".to_owned(),
        DataType::Int(_) | DataType::Float(_) => "number".to_owned(),
        DataType::Boolean(_) => "boolean".to_owned(),
        DataType::List(inner, _) => format!("{}[]", render_ts_type(inner)),
        DataType::Custom(name, _) => name.clone(),
    }
}

fn render_zod_type(data_type: &DataType) -> String {
    match data_type {
        DataType::String(_) => "z.string()".to_owned(),
        DataType::Int(_) => "z.number().int()".to_owned(),
        DataType::Float(_) => "z.number()".to_owned(),
        DataType::Boolean(_) => "z.boolean()".to_owned(),
        DataType::List(inner, _) => format!("z.array({})", render_zod_type(inner)),
        DataType::Custom(name, _) => format!("{name}Schema"),
    }
}

fn apply_constraints(base: String, constraints: &[Constraint]) -> CompilerResult<String> {
    let mut schema = base;

    for constraint in constraints {
        schema = match constraint.name.as_str() {
            "min" => format!(
                "{schema}.min({})",
                render_numeric_constraint_value(constraint)?
            ),
            "max" => format!(
                "{schema}.max({})",
                render_numeric_constraint_value(constraint)?
            ),
            "regex" => format!("{schema}.regex({})", render_regex_constraint_value(constraint)?),
            other => {
                return Err(CompilerError::UnsupportedConstraint {
                    name: other.to_owned(),
                    span: constraint.span.clone(),
                })
            }
        };
    }

    Ok(schema)
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

fn render_regex_constraint_value(constraint: &Constraint) -> CompilerResult<String> {
    match &constraint.value.expr {
        Expr::StringLiteral(pattern) => Ok(format!("new RegExp({pattern:?})")),
        _ => Err(CompilerError::InvalidConstraintValue {
            name: constraint.name.clone(),
            expected: "a string literal".to_owned(),
            span: constraint.span.clone(),
        }),
    }
}

fn trim_float(value: f64) -> String {
    let mut rendered = value.to_string();
    if rendered.ends_with(".0") {
        rendered.truncate(rendered.len() - 2);
    }
    rendered
}
