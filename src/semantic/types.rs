use std::collections::HashMap;

use crate::ast::{
    AgentDecl, Block, DataType, Document, ElseBranch, Expr, MockDecl, SpannedExpr, Statement,
    TestDecl, TypeField, WorkflowDecl,
};
use crate::errors::{CompilerError, CompilerResult};

use super::symbols::SymbolTable;

#[derive(Debug, Clone, PartialEq, Eq)]
enum TypeShape {
    String,
    Int,
    Float,
    Boolean,
    List(Box<TypeShape>),
    Custom(String),
}

type TypeEnv = HashMap<String, TypeShape>;

pub(crate) fn validate_references(
    document: &Document,
    symbols: &SymbolTable,
) -> CompilerResult<()> {
    validate_declared_types(document, symbols)?;

    for agent in &document.agents {
        validate_agent_references(agent, symbols)?;
    }

    for workflow in &document.workflows {
        validate_block_references(&workflow.body, symbols)?;
    }

    for listener in &document.listeners {
        validate_block_references(&listener.body, symbols)?;
    }

    for test in &document.tests {
        validate_test_references(test, symbols)?;
    }

    for mock in &document.mocks {
        validate_mock_references(mock, symbols)?;
    }

    Ok(())
}

pub(crate) fn validate_types(document: &Document, symbols: &SymbolTable) -> CompilerResult<()> {
    for workflow in &document.workflows {
        validate_workflow_types(workflow, symbols)?;
    }

    for listener in &document.listeners {
        let mut env = TypeEnv::new();
        validate_block_types(&listener.body, symbols, &mut env, None)?;
    }

    for test in &document.tests {
        let mut env = TypeEnv::new();
        validate_block_types(&test.body, symbols, &mut env, None)?;
    }

    Ok(())
}

fn validate_declared_types(document: &Document, symbols: &SymbolTable) -> CompilerResult<()> {
    for declaration in &document.types {
        validate_type_fields(&declaration.fields, symbols)?;
    }

    for declaration in &document.tools {
        validate_type_fields(&declaration.arguments, symbols)?;

        if let Some(return_type) = &declaration.return_type {
            ensure_declared_type_exists(return_type, symbols)?;
        }
    }

    for declaration in &document.workflows {
        validate_type_fields(&declaration.arguments, symbols)?;

        if let Some(return_type) = &declaration.return_type {
            ensure_declared_type_exists(return_type, symbols)?;
        }
    }

    Ok(())
}

fn validate_type_fields(fields: &[TypeField], symbols: &SymbolTable) -> CompilerResult<()> {
    for field in fields {
        ensure_declared_type_exists(&field.data_type, symbols)?;
    }

    Ok(())
}

fn ensure_declared_type_exists(data_type: &DataType, symbols: &SymbolTable) -> CompilerResult<()> {
    match data_type {
        DataType::String(_)
        | DataType::Int(_)
        | DataType::Float(_)
        | DataType::Boolean(_) => Ok(()),
        DataType::List(inner, _) => ensure_declared_type_exists(inner, symbols),
        DataType::Custom(name, span) => {
            if symbols.has_type(name) {
                Ok(())
            } else {
                Err(CompilerError::UndefinedType {
                    name: name.clone(),
                    span: span.clone(),
                })
            }
        }
    }
}

fn validate_agent_references(agent: &AgentDecl, symbols: &SymbolTable) -> CompilerResult<()> {
    if let Some(name) = &agent.extends {
        ensure_agent_exists(name, &agent.span, symbols)?;
    }

    if let Some(name) = &agent.client {
        if !symbols.has_client(name) {
            return Err(CompilerError::UndefinedClient {
                name: name.clone(),
                span: agent.span.clone(),
            });
        }
    }

    for tool in &agent.tools {
        if !symbols.has_tool(tool) {
            return Err(CompilerError::UndefinedTool {
                name: tool.clone(),
                span: agent.span.clone(),
            });
        }
    }

    Ok(())
}

fn validate_test_references(test: &TestDecl, symbols: &SymbolTable) -> CompilerResult<()> {
    validate_block_references(&test.body, symbols)
}

fn validate_mock_references(mock: &MockDecl, symbols: &SymbolTable) -> CompilerResult<()> {
    ensure_agent_exists(&mock.target_agent, &mock.span, symbols)?;
    for (_, value) in &mock.output {
        validate_spanned_expr_references(value, symbols)?;
    }
    Ok(())
}

fn validate_block_references(block: &Block, symbols: &SymbolTable) -> CompilerResult<()> {
    for statement in &block.statements {
        validate_statement_references(statement, symbols)?;
    }

    Ok(())
}

fn validate_statement_references(
    statement: &Statement,
    symbols: &SymbolTable,
) -> CompilerResult<()> {
    match statement {
        Statement::LetDecl {
            explicit_type,
            value,
            ..
        } => {
            if let Some(data_type) = explicit_type {
                ensure_declared_type_exists(data_type, symbols)?;
            }

            validate_spanned_expr_references(value, symbols)
        }
        Statement::ForLoop { iterator, body, .. } => {
            validate_spanned_expr_references(iterator, symbols)?;
            validate_block_references(body, symbols)
        }
        Statement::IfCond {
            condition,
            if_body,
            else_body,
            ..
        } => {
            validate_spanned_expr_references(condition, symbols)?;
            validate_block_references(if_body, symbols)?;

            match else_body {
                Some(ElseBranch::Else(block)) => validate_block_references(block, symbols)?,
                Some(ElseBranch::ElseIf(stmt)) => validate_statement_references(stmt, symbols)?,
                None => {}
            }

            Ok(())
        }
        Statement::ExecuteRun {
            agent_name,
            kwargs,
            require_type,
            span,
        } => validate_execute_references(agent_name, kwargs, require_type.as_ref(), symbols, span),
        Statement::Return { value, .. } => validate_spanned_expr_references(value, symbols),
        Statement::Expression(spanned) => validate_spanned_expr_references(spanned, symbols),
        Statement::TryCatch {
            try_body,
            catch_type,
            catch_body,
            ..
        } => {
            validate_block_references(try_body, symbols)?;
            ensure_declared_type_exists(catch_type, symbols)?;
            validate_block_references(catch_body, symbols)
        }
        Statement::Assert { condition, .. } => validate_spanned_expr_references(condition, symbols),
        Statement::Continue(_) | Statement::Break(_) => Ok(()),
    }
}

fn validate_spanned_expr_references(
    spanned: &SpannedExpr,
    symbols: &SymbolTable,
) -> CompilerResult<()> {
    validate_expr_references(&spanned.expr, symbols, &spanned.span)
}

fn validate_expr_references(
    expr: &Expr,
    symbols: &SymbolTable,
    span: &std::ops::Range<usize>,
) -> CompilerResult<()> {
    match expr {
        Expr::ArrayLiteral(values) | Expr::Call(_, values) => {
            for value in values {
                validate_spanned_expr_references(value, symbols)?;
            }

            Ok(())
        }
        Expr::MemberAccess(target, _) => validate_spanned_expr_references(target, symbols),
        Expr::MethodCall(target, _, args) => {
            validate_spanned_expr_references(target, symbols)?;

            for arg in args {
                validate_spanned_expr_references(arg, symbols)?;
            }

            Ok(())
        }
        Expr::ExecuteRun {
            agent_name,
            kwargs,
            require_type,
        } => validate_execute_references(agent_name, kwargs, require_type.as_ref(), symbols, span),
        Expr::BinaryOp { left, right, .. } => {
            validate_spanned_expr_references(left, symbols)?;
            validate_spanned_expr_references(right, symbols)
        }
        Expr::StringLiteral(_)
        | Expr::IntLiteral(_)
        | Expr::FloatLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Identifier(_) => Ok(()),
    }
}

fn validate_execute_references(
    agent_name: &str,
    kwargs: &[(String, SpannedExpr)],
    require_type: Option<&DataType>,
    symbols: &SymbolTable,
    span: &std::ops::Range<usize>,
) -> CompilerResult<()> {
    ensure_agent_exists(agent_name, span, symbols)?;

    for (_, value) in kwargs {
        validate_spanned_expr_references(value, symbols)?;
    }

    if let Some(data_type) = require_type {
        ensure_declared_type_exists(data_type, symbols)?;
    }

    Ok(())
}

fn ensure_agent_exists(
    agent_name: &str,
    span: &std::ops::Range<usize>,
    symbols: &SymbolTable,
) -> CompilerResult<()> {
    if symbols.has_agent(agent_name) {
        Ok(())
    } else {
        Err(CompilerError::UndefinedAgent {
            name: agent_name.to_owned(),
            span: span.clone(),
        })
    }
}

fn validate_workflow_types(workflow: &WorkflowDecl, symbols: &SymbolTable) -> CompilerResult<()> {
    let mut env = seed_workflow_env(workflow);
    let return_type = workflow.return_type.as_ref().map(type_shape_from_data_type);
    validate_block_types(&workflow.body, symbols, &mut env, return_type.as_ref())
}

fn seed_workflow_env(workflow: &WorkflowDecl) -> TypeEnv {
    workflow
        .arguments
        .iter()
        .map(|argument| {
            (
                argument.name.clone(),
                type_shape_from_data_type(&argument.data_type),
            )
        })
        .collect()
}

fn validate_block_types(
    block: &Block,
    symbols: &SymbolTable,
    env: &mut TypeEnv,
    return_type: Option<&TypeShape>,
) -> CompilerResult<()> {
    for statement in &block.statements {
        validate_statement_types(statement, symbols, env, return_type)?;
    }

    Ok(())
}

fn validate_statement_types(
    statement: &Statement,
    symbols: &SymbolTable,
    env: &mut TypeEnv,
    return_type: Option<&TypeShape>,
) -> CompilerResult<()> {
    match statement {
        Statement::LetDecl {
            name,
            explicit_type,
            value,
            span,
        } => validate_let_statement(name, explicit_type.as_ref(), &value.expr, span, symbols, env),
        Statement::ForLoop {
            item_name,
            iterator,
            body,
            ..
        } => validate_for_loop(item_name, iterator, body, symbols, env, return_type),
        Statement::IfCond {
            condition,
            if_body,
            else_body,
            span,
        } => validate_if_statement(&condition.expr, if_body, else_body.as_ref(), span, symbols, env, return_type),
        Statement::ExecuteRun {
            agent_name,
            kwargs,
            require_type,
            span,
        } => {
            let _ = infer_execute_type(agent_name, kwargs, require_type.as_ref(), span, symbols, env)?;
            Ok(())
        }
        Statement::Return { value, span } => validate_return_statement(&value.expr, span, symbols, env, return_type),
        Statement::Expression(spanned) => {
            let _ = infer_expr_type(&spanned.expr, &spanned.span, symbols, env)?;
            Ok(())
        }
        Statement::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            validate_block_types(try_body, symbols, &mut env.clone(), return_type)?;
            validate_block_types(catch_body, symbols, &mut env.clone(), return_type)
        }
        Statement::Assert { condition, span, .. } => {
            if let Some(condition_type) = infer_expr_type(&condition.expr, span, symbols, env)? {
                ensure_types_match(&TypeShape::Boolean, &condition_type, span)?;
            }
            Ok(())
        }
        Statement::Continue(_) | Statement::Break(_) => Ok(()),
    }
}

fn validate_let_statement(
    name: &str,
    explicit_type: Option<&DataType>,
    value: &Expr,
    span: &std::ops::Range<usize>,
    symbols: &SymbolTable,
    env: &mut TypeEnv,
) -> CompilerResult<()> {
    let expected = explicit_type.map(type_shape_from_data_type);
    let found = infer_expr_type(value, span, symbols, env)?;

    if let (Some(expected), Some(found)) = (expected.as_ref(), found.as_ref()) {
        ensure_types_match(expected, found, span)?;
    }

    if let Some(shape) = expected.or(found) {
        env.insert(name.to_owned(), shape);
    }

    Ok(())
}

fn validate_for_loop(
    item_name: &str,
    iterator: &SpannedExpr,
    body: &Block,
    symbols: &SymbolTable,
    env: &TypeEnv,
    return_type: Option<&TypeShape>,
) -> CompilerResult<()> {
    let mut nested_env = env.clone();

    // If the iterator is a simple identifier, look up its type
    if let Expr::Identifier(iterator_name) = &iterator.expr {
        if let Some(TypeShape::List(item_type)) = env.get(iterator_name).cloned() {
            nested_env.insert(item_name.to_owned(), *item_type);
        }
    }

    validate_block_types(body, symbols, &mut nested_env, return_type)
}

fn validate_if_statement(
    condition: &Expr,
    if_body: &Block,
    else_body: Option<&ElseBranch>,
    span: &std::ops::Range<usize>,
    symbols: &SymbolTable,
    env: &TypeEnv,
    return_type: Option<&TypeShape>,
) -> CompilerResult<()> {
    if let Some(condition_type) = infer_expr_type(condition, span, symbols, env)? {
        ensure_types_match(&TypeShape::Boolean, &condition_type, span)?;
    }

    let mut if_env = env.clone();
    validate_block_types(if_body, symbols, &mut if_env, return_type)?;

    match else_body {
        Some(ElseBranch::Else(block)) => {
            let mut else_env = env.clone();
            validate_block_types(block, symbols, &mut else_env, return_type)?;
        }
        Some(ElseBranch::ElseIf(stmt)) => {
            validate_statement_types(stmt, symbols, &mut env.clone(), return_type)?;
        }
        None => {}
    }

    Ok(())
}

fn validate_return_statement(
    value: &Expr,
    span: &std::ops::Range<usize>,
    symbols: &SymbolTable,
    env: &TypeEnv,
    return_type: Option<&TypeShape>,
) -> CompilerResult<()> {
    if let Some(expected) = return_type {
        if let Some(found) = infer_expr_type(value, span, symbols, env)? {
            ensure_types_match(expected, &found, span)?;
        }
    } else {
        let _ = infer_expr_type(value, span, symbols, env)?;
    }

    Ok(())
}

fn infer_expr_type(
    expr: &Expr,
    span: &std::ops::Range<usize>,
    symbols: &SymbolTable,
    env: &TypeEnv,
) -> CompilerResult<Option<TypeShape>> {
    match expr {
        Expr::StringLiteral(_) => Ok(Some(TypeShape::String)),
        Expr::IntLiteral(_) => Ok(Some(TypeShape::Int)),
        Expr::FloatLiteral(_) => Ok(Some(TypeShape::Float)),
        Expr::BoolLiteral(_) => Ok(Some(TypeShape::Boolean)),
        Expr::Identifier(name) => Ok(env.get(name).cloned()),
        Expr::ArrayLiteral(values) => infer_array_type(values, span, symbols, env),
        Expr::Call(_, args) => {
            validate_spanned_expr_list(args, symbols, env)?;
            Ok(None)
        }
        Expr::MemberAccess(target, _) => {
            let _ = infer_expr_type(&target.expr, &target.span, symbols, env)?;
            Ok(None)
        }
        Expr::MethodCall(target, _, args) => {
            let _ = infer_expr_type(&target.expr, &target.span, symbols, env)?;
            validate_spanned_expr_list(args, symbols, env)?;
            Ok(None)
        }
        Expr::ExecuteRun {
            agent_name,
            kwargs,
            require_type,
        } => infer_execute_type(agent_name, kwargs, require_type.as_ref(), span, symbols, env),
        Expr::BinaryOp { left, right, .. } => {
            infer_binary_operand_types(left, right, symbols, env)?;
            Ok(Some(TypeShape::Boolean))
        }
    }
}

fn validate_spanned_expr_list(
    expressions: &[SpannedExpr],
    symbols: &SymbolTable,
    env: &TypeEnv,
) -> CompilerResult<()> {
    for expression in expressions {
        let _ = infer_expr_type(&expression.expr, &expression.span, symbols, env)?;
    }

    Ok(())
}

fn infer_array_type(
    values: &[SpannedExpr],
    span: &std::ops::Range<usize>,
    symbols: &SymbolTable,
    env: &TypeEnv,
) -> CompilerResult<Option<TypeShape>> {
    let mut item_type = None;

    for value in values {
        if let Some(found) = infer_expr_type(&value.expr, &value.span, symbols, env)? {
            if let Some(expected) = item_type.as_ref() {
                ensure_types_match(expected, &found, span)?;
            } else {
                item_type = Some(found);
            }
        }
    }

    Ok(item_type.map(|shape| TypeShape::List(Box::new(shape))))
}

fn infer_execute_type(
    agent_name: &str,
    kwargs: &[(String, SpannedExpr)],
    require_type: Option<&DataType>,
    span: &std::ops::Range<usize>,
    symbols: &SymbolTable,
    env: &TypeEnv,
) -> CompilerResult<Option<TypeShape>> {
    ensure_agent_exists(agent_name, span, symbols)?;

    for (_, value) in kwargs {
        let _ = infer_expr_type(&value.expr, &value.span, symbols, env)?;
    }

    Ok(require_type.map(type_shape_from_data_type))
}

fn infer_binary_operand_types(
    left: &SpannedExpr,
    right: &SpannedExpr,
    symbols: &SymbolTable,
    env: &TypeEnv,
) -> CompilerResult<()> {
    let left_type = infer_expr_type(&left.expr, &left.span, symbols, env)?;
    let right_type = infer_expr_type(&right.expr, &right.span, symbols, env)?;

    if let (Some(left_type), Some(right_type)) = (left_type, right_type) {
        ensure_types_match(&left_type, &right_type, &left.span)?;
    }

    Ok(())
}

fn ensure_types_match(
    expected: &TypeShape,
    found: &TypeShape,
    span: &std::ops::Range<usize>,
) -> CompilerResult<()> {
    if expected == found {
        Ok(())
    } else {
        Err(CompilerError::TypeMismatch {
            expected: expected.display(),
            found: found.display(),
            span: span.clone(),
        })
    }
}

fn type_shape_from_data_type(data_type: &DataType) -> TypeShape {
    match data_type {
        DataType::String(_) => TypeShape::String,
        DataType::Int(_) => TypeShape::Int,
        DataType::Float(_) => TypeShape::Float,
        DataType::Boolean(_) => TypeShape::Boolean,
        DataType::List(inner, _) => TypeShape::List(Box::new(type_shape_from_data_type(inner))),
        DataType::Custom(name, _) => TypeShape::Custom(name.clone()),
    }
}

impl TypeShape {
    fn display(&self) -> String {
        match self {
            Self::String => "string".to_owned(),
            Self::Int => "int".to_owned(),
            Self::Float => "float".to_owned(),
            Self::Boolean => "boolean".to_owned(),
            Self::List(inner) => format!("list<{}>", inner.display()),
            Self::Custom(name) => name.clone(),
        }
    }
}
