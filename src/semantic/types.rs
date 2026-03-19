use std::collections::HashMap;

use crate::ast::{
    AgentDecl, BinaryOp, Block, DataType, Document, ElseBranch, Expr, MockDecl, Span,
    SpannedExpr, Statement, TestDecl, TypeField, WorkflowDecl,
};
use crate::errors::CompilerError;

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

#[derive(Debug, Clone, Copy, Default)]
struct FlowContext {
    in_test: bool,
    loop_depth: usize,
}

impl FlowContext {
    fn in_test(self) -> Self {
        Self {
            in_test: true,
            ..self
        }
    }

    fn enter_loop(self) -> Self {
        Self {
            loop_depth: self.loop_depth + 1,
            ..self
        }
    }
}

#[derive(Clone, Copy)]
struct TypeCheckContext<'a> {
    symbols: &'a SymbolTable,
    return_type: Option<&'a TypeShape>,
    flow: FlowContext,
}

impl<'a> TypeCheckContext<'a> {
    fn enter_loop(self) -> Self {
        Self {
            flow: self.flow.enter_loop(),
            ..self
        }
    }
}

pub(crate) fn validate_references_collecting(
    document: &Document,
    symbols: &SymbolTable,
) -> Vec<CompilerError> {
    let mut errors = Vec::new();

    validate_declared_types_collecting(document, symbols, &mut errors);

    for agent in &document.agents {
        validate_agent_references_collecting(agent, symbols, &mut errors);
    }

    for workflow in &document.workflows {
        validate_block_references_collecting(
            &workflow.body,
            symbols,
            FlowContext::default(),
            &mut errors,
        );
    }

    for listener in &document.listeners {
        validate_block_references_collecting(
            &listener.body,
            symbols,
            FlowContext::default(),
            &mut errors,
        );
    }

    for test in &document.tests {
        validate_test_references_collecting(test, symbols, &mut errors);
    }

    for mock in &document.mocks {
        validate_mock_references_collecting(mock, symbols, &mut errors);
    }

    errors
}

pub(crate) fn validate_types_collecting(
    document: &Document,
    symbols: &SymbolTable,
) -> Vec<CompilerError> {
    let mut errors = Vec::new();

    for workflow in &document.workflows {
        validate_workflow_types_collecting(workflow, symbols, &mut errors);
    }

    for listener in &document.listeners {
        let mut env = TypeEnv::new();
        let context = TypeCheckContext {
            symbols,
            return_type: None,
            flow: FlowContext::default(),
        };
        validate_block_types_collecting(
            &listener.body,
            &mut env,
            context,
            &mut errors,
        );
    }

    for test in &document.tests {
        let mut env = TypeEnv::new();
        let context = TypeCheckContext {
            symbols,
            return_type: None,
            flow: FlowContext::default().in_test(),
        };
        validate_block_types_collecting(
            &test.body,
            &mut env,
            context,
            &mut errors,
        );
    }

    errors
}
fn validate_declared_types_collecting(
    document: &Document,
    symbols: &SymbolTable,
    errors: &mut Vec<CompilerError>,
) {
    for declaration in &document.types {
        validate_type_fields_collecting(&declaration.fields, symbols, errors);
    }

    for declaration in &document.tools {
        validate_type_fields_collecting(&declaration.arguments, symbols, errors);

        if let Some(return_type) = &declaration.return_type {
            collect_error(errors, ensure_declared_type_exists(return_type, symbols));
        }
    }

    for declaration in &document.workflows {
        validate_type_fields_collecting(&declaration.arguments, symbols, errors);

        if let Some(return_type) = &declaration.return_type {
            collect_error(errors, ensure_declared_type_exists(return_type, symbols));
        }
    }
}

fn validate_type_fields_collecting(
    fields: &[TypeField],
    symbols: &SymbolTable,
    errors: &mut Vec<CompilerError>,
) {
    for field in fields {
        collect_error(errors, ensure_declared_type_exists(&field.data_type, symbols));
    }
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

fn validate_agent_references_collecting(
    agent: &AgentDecl,
    symbols: &SymbolTable,
    errors: &mut Vec<CompilerError>,
) {
    if let Some(name) = &agent.extends {
        collect_error(errors, ensure_agent_exists(name, &agent.span, symbols));
    }

    if let Some(name) = &agent.client {
        if !symbols.has_client(name) {
            errors.push(CompilerError::UndefinedClient {
                name: name.clone(),
                span: agent.span.clone(),
            });
        }
    }

    for tool in &agent.tools {
        if !symbols.has_tool(tool) {
            errors.push(CompilerError::UndefinedTool {
                name: tool.clone(),
                span: agent.span.clone(),
            });
        }
    }
}

fn validate_test_references_collecting(
    test: &TestDecl,
    symbols: &SymbolTable,
    errors: &mut Vec<CompilerError>,
) {
    validate_block_references_collecting(&test.body, symbols, FlowContext::default().in_test(), errors);
}

fn validate_mock_references_collecting(
    mock: &MockDecl,
    symbols: &SymbolTable,
    errors: &mut Vec<CompilerError>,
) {
    collect_error(errors, ensure_agent_exists(&mock.target_agent, &mock.span, symbols));

    for (_, value) in &mock.output {
        validate_spanned_expr_references_collecting(value, symbols, errors);
    }
}

fn validate_block_references_collecting(
    block: &Block,
    symbols: &SymbolTable,
    context: FlowContext,
    errors: &mut Vec<CompilerError>,
) {
    for statement in &block.statements {
        validate_statement_references_collecting(statement, symbols, context, errors);
    }
}

fn validate_statement_references_collecting(
    statement: &Statement,
    symbols: &SymbolTable,
    context: FlowContext,
    errors: &mut Vec<CompilerError>,
) {
    match statement {
        Statement::LetDecl {
            explicit_type,
            value,
            ..
        } => {
            if let Some(data_type) = explicit_type {
                collect_error(errors, ensure_declared_type_exists(data_type, symbols));
            }

            validate_spanned_expr_references_collecting(value, symbols, errors);
        }
        Statement::ForLoop { iterator, body, .. } => {
            validate_spanned_expr_references_collecting(iterator, symbols, errors);
            validate_block_references_collecting(body, symbols, context.enter_loop(), errors);
        }
        Statement::IfCond {
            condition,
            if_body,
            else_body,
            ..
        } => {
            validate_spanned_expr_references_collecting(condition, symbols, errors);
            validate_block_references_collecting(if_body, symbols, context, errors);

            if let Some(else_body) = else_body {
                validate_else_branch_references_collecting(else_body, symbols, context, errors);
            }
        }
        Statement::ExecuteRun {
            agent_name,
            kwargs,
            require_type,
            span,
        } => validate_execute_references_collecting(
            agent_name,
            kwargs,
            require_type.as_ref(),
            span,
            symbols,
            errors,
        ),
        Statement::Return { value, .. } => {
            validate_spanned_expr_references_collecting(value, symbols, errors);
        }
        Statement::Expression(spanned) => {
            validate_spanned_expr_references_collecting(spanned, symbols, errors);
        }
        Statement::TryCatch {
            try_body,
            catch_type,
            catch_body,
            ..
        } => {
            validate_block_references_collecting(try_body, symbols, context, errors);
            collect_error(errors, ensure_declared_type_exists(catch_type, symbols));
            validate_block_references_collecting(catch_body, symbols, context, errors);
        }
        Statement::Assert {
            condition, span, ..
        } => {
            if !context.in_test {
                errors.push(CompilerError::InvalidAssertOutsideTest { span: span.clone() });
            }

            validate_spanned_expr_references_collecting(condition, symbols, errors);
        }
        Statement::Continue(_) | Statement::Break(_) => {}
    }
}

fn validate_else_branch_references_collecting(
    else_branch: &ElseBranch,
    symbols: &SymbolTable,
    context: FlowContext,
    errors: &mut Vec<CompilerError>,
) {
    match else_branch {
        ElseBranch::Else(block) => {
            validate_block_references_collecting(block, symbols, context, errors);
        }
        ElseBranch::ElseIf(statement) => {
            validate_statement_references_collecting(statement, symbols, context, errors);
        }
    }
}

fn validate_spanned_expr_references_collecting(
    spanned: &SpannedExpr,
    symbols: &SymbolTable,
    errors: &mut Vec<CompilerError>,
) {
    validate_expr_references_collecting(&spanned.expr, &spanned.span, symbols, errors);
}

fn validate_expr_references_collecting(
    expr: &Expr,
    span: &Span,
    symbols: &SymbolTable,
    errors: &mut Vec<CompilerError>,
) {
    match expr {
        Expr::ArrayLiteral(values) | Expr::Call(_, values) => {
            for value in values {
                validate_spanned_expr_references_collecting(value, symbols, errors);
            }
        }
        Expr::MemberAccess(target, _) => {
            validate_spanned_expr_references_collecting(target, symbols, errors);
        }
        Expr::MethodCall(target, _, args) => {
            validate_spanned_expr_references_collecting(target, symbols, errors);

            for arg in args {
                validate_spanned_expr_references_collecting(arg, symbols, errors);
            }
        }
        Expr::ExecuteRun {
            agent_name,
            kwargs,
            require_type,
        } => validate_execute_references_collecting(
            agent_name,
            kwargs,
            require_type.as_ref(),
            span,
            symbols,
            errors,
        ),
        Expr::BinaryOp { left, right, .. } => {
            validate_spanned_expr_references_collecting(left, symbols, errors);
            validate_spanned_expr_references_collecting(right, symbols, errors);
        }
        Expr::StringLiteral(_)
        | Expr::IntLiteral(_)
        | Expr::FloatLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Identifier(_) => {}
    }
}

fn validate_execute_references_collecting(
    agent_name: &str,
    kwargs: &[(String, SpannedExpr)],
    require_type: Option<&DataType>,
    span: &Span,
    symbols: &SymbolTable,
    errors: &mut Vec<CompilerError>,
) {
    collect_error(errors, ensure_agent_exists(agent_name, span, symbols));

    for (_, value) in kwargs {
        validate_spanned_expr_references_collecting(value, symbols, errors);
    }

    if let Some(data_type) = require_type {
        collect_error(errors, ensure_declared_type_exists(data_type, symbols));
    }
}

fn ensure_agent_exists(agent_name: &str, span: &Span, symbols: &SymbolTable) -> CompilerResult<()> {
    if symbols.has_agent(agent_name) {
        Ok(())
    } else {
        Err(CompilerError::UndefinedAgent {
            name: agent_name.to_owned(),
            span: span.clone(),
        })
    }
}

type CompilerResult<T> = Result<T, CompilerError>;

fn validate_workflow_types_collecting(
    workflow: &WorkflowDecl,
    symbols: &SymbolTable,
    errors: &mut Vec<CompilerError>,
) {
    let mut env = seed_workflow_env(workflow, symbols);
    let return_type = workflow
        .return_type
        .as_ref()
        .and_then(|data_type| known_type_shape(data_type, symbols));
    let context = TypeCheckContext {
        symbols,
        return_type: return_type.as_ref(),
        flow: FlowContext::default(),
    };
    validate_block_types_collecting(
        &workflow.body,
        &mut env,
        context,
        errors,
    );
}

fn seed_workflow_env(workflow: &WorkflowDecl, symbols: &SymbolTable) -> TypeEnv {
    workflow
        .arguments
        .iter()
        .filter_map(|argument| {
            known_type_shape(&argument.data_type, symbols)
                .map(|shape| (argument.name.clone(), shape))
        })
        .collect()
}

fn validate_block_types_collecting(
    block: &Block,
    env: &mut TypeEnv,
    context: TypeCheckContext<'_>,
    errors: &mut Vec<CompilerError>,
) {
    for statement in &block.statements {
        validate_statement_types_collecting(statement, env, context, errors);
    }
}

fn validate_statement_types_collecting(
    statement: &Statement,
    env: &mut TypeEnv,
    context: TypeCheckContext<'_>,
    errors: &mut Vec<CompilerError>,
) {
    match statement {
        Statement::LetDecl {
            name,
            explicit_type,
            value,
            span,
        } => validate_let_statement_collecting(
            name,
            explicit_type.as_ref(),
            value,
            span,
            context.symbols,
            env,
            errors,
        ),
        Statement::ForLoop {
            item_name,
            iterator,
            body,
            ..
        } => validate_for_loop_collecting(item_name, iterator, body, env, context, errors),
        Statement::IfCond {
            condition,
            if_body,
            else_body,
            span,
        } => validate_if_statement_collecting(
            condition,
            if_body,
            else_body.as_ref(),
            span,
            env,
            context,
            errors,
        ),
        Statement::ExecuteRun {
            agent_name,
            kwargs,
            require_type,
            span,
        } => {
            infer_execute_type_collecting(
                agent_name,
                kwargs,
                require_type.as_ref(),
                span,
                context.symbols,
                env,
                errors,
            );
        }
        Statement::Return { value, span } => {
            validate_return_statement_collecting(value, span, env, context, errors);
        }
        Statement::Expression(spanned) => {
            infer_expr_type_collecting(spanned, context.symbols, env, errors);
        }
        Statement::TryCatch {
            try_body,
            catch_name,
            catch_type,
            catch_body,
            ..
        } => validate_try_catch_collecting(
            try_body,
            catch_name,
            catch_type,
            catch_body,
            env,
            context,
            errors,
        ),
        Statement::Assert {
            condition, span, ..
        } => validate_assert_statement_collecting(condition, span, context.symbols, env, errors),
        Statement::Continue(span) => validate_control_flow_collecting("continue", span, context.flow, errors),
        Statement::Break(span) => validate_control_flow_collecting("break", span, context.flow, errors),
    }
}

fn validate_let_statement_collecting(
    name: &str,
    explicit_type: Option<&DataType>,
    value: &SpannedExpr,
    span: &Span,
    symbols: &SymbolTable,
    env: &mut TypeEnv,
    errors: &mut Vec<CompilerError>,
) {
    let expected = explicit_type.and_then(|data_type| known_type_shape(data_type, symbols));
    let found = infer_expr_type_collecting(value, symbols, env, errors);

    if let (Some(expected), Some(found)) = (expected.as_ref(), found.as_ref()) {
        ensure_types_match_collecting(expected, found, span, errors);
    }

    if let Some(shape) = expected.or(found) {
        env.insert(name.to_owned(), shape);
    }
}

fn validate_for_loop_collecting(
    item_name: &str,
    iterator: &SpannedExpr,
    body: &Block,
    env: &TypeEnv,
    context: TypeCheckContext<'_>,
    errors: &mut Vec<CompilerError>,
) {
    let mut nested_env = env.clone();
    let iterator_type = infer_expr_type_collecting(iterator, context.symbols, env, errors);

    if let Some(TypeShape::List(item_type)) = iterator_type {
        nested_env.insert(item_name.to_owned(), *item_type);
    }

    validate_block_types_collecting(
        body,
        &mut nested_env,
        context.enter_loop(),
        errors,
    );
}

fn validate_if_statement_collecting(
    condition: &SpannedExpr,
    if_body: &Block,
    else_body: Option<&ElseBranch>,
    span: &Span,
    env: &TypeEnv,
    context: TypeCheckContext<'_>,
    errors: &mut Vec<CompilerError>,
) {
    if let Some(condition_type) = infer_expr_type_collecting(condition, context.symbols, env, errors) {
        ensure_types_match_collecting(&TypeShape::Boolean, &condition_type, span, errors);
    }

    let mut if_env = env.clone();
    validate_block_types_collecting(if_body, &mut if_env, context, errors);

    if let Some(else_body) = else_body {
        validate_else_branch_types_collecting(else_body, env, context, errors);
    }
}

fn validate_else_branch_types_collecting(
    else_branch: &ElseBranch,
    env: &TypeEnv,
    context: TypeCheckContext<'_>,
    errors: &mut Vec<CompilerError>,
) {
    match else_branch {
        ElseBranch::Else(block) => {
            let mut else_env = env.clone();
            validate_block_types_collecting(block, &mut else_env, context, errors);
        }
        ElseBranch::ElseIf(statement) => {
            let mut else_if_env = env.clone();
            validate_statement_types_collecting(statement, &mut else_if_env, context, errors);
        }
    }
}

fn validate_return_statement_collecting(
    value: &SpannedExpr,
    span: &Span,
    env: &TypeEnv,
    context: TypeCheckContext<'_>,
    errors: &mut Vec<CompilerError>,
) {
    if let Some(expected) = context.return_type {
        if let Some(found) = infer_expr_type_collecting(value, context.symbols, env, errors) {
            ensure_types_match_collecting(expected, &found, span, errors);
        }
    } else {
        infer_expr_type_collecting(value, context.symbols, env, errors);
    }
}

fn validate_try_catch_collecting(
    try_body: &Block,
    catch_name: &str,
    catch_type: &DataType,
    catch_body: &Block,
    env: &TypeEnv,
    context: TypeCheckContext<'_>,
    errors: &mut Vec<CompilerError>,
) {
    let mut try_env = env.clone();
    validate_block_types_collecting(try_body, &mut try_env, context, errors);

    let mut catch_env = env.clone();
    if let Some(catch_type) = known_type_shape(catch_type, context.symbols) {
        catch_env.insert(catch_name.to_owned(), catch_type);
    }
    validate_block_types_collecting(catch_body, &mut catch_env, context, errors);
}

fn validate_assert_statement_collecting(
    condition: &SpannedExpr,
    span: &Span,
    symbols: &SymbolTable,
    env: &TypeEnv,
    errors: &mut Vec<CompilerError>,
) {
    if let Some(condition_type) = infer_expr_type_collecting(condition, symbols, env, errors) {
        ensure_types_match_collecting(&TypeShape::Boolean, &condition_type, span, errors);
    }
}

fn validate_control_flow_collecting(
    keyword: &str,
    span: &Span,
    context: FlowContext,
    errors: &mut Vec<CompilerError>,
) {
    if context.loop_depth == 0 {
        errors.push(CompilerError::InvalidControlFlow {
            keyword: keyword.to_owned(),
            span: span.clone(),
        });
    }
}

fn infer_expr_type_collecting(
    spanned: &SpannedExpr,
    symbols: &SymbolTable,
    env: &TypeEnv,
    errors: &mut Vec<CompilerError>,
) -> Option<TypeShape> {
    match &spanned.expr {
        Expr::StringLiteral(_) => Some(TypeShape::String),
        Expr::IntLiteral(_) => Some(TypeShape::Int),
        Expr::FloatLiteral(_) => Some(TypeShape::Float),
        Expr::BoolLiteral(_) => Some(TypeShape::Boolean),
        Expr::Identifier(name) => env.get(name).cloned(),
        Expr::ArrayLiteral(values) => infer_array_type_collecting(values, &spanned.span, symbols, env, errors),
        Expr::Call(_, args) => {
            validate_spanned_expr_list_collecting(args, symbols, env, errors);
            None
        }
        Expr::MemberAccess(target, _) => {
            infer_expr_type_collecting(target, symbols, env, errors);
            None
        }
        Expr::MethodCall(target, _, args) => {
            infer_expr_type_collecting(target, symbols, env, errors);
            validate_spanned_expr_list_collecting(args, symbols, env, errors);
            None
        }
        Expr::ExecuteRun {
            agent_name,
            kwargs,
            require_type,
        } => infer_execute_type_collecting(
            agent_name,
            kwargs,
            require_type.as_ref(),
            &spanned.span,
            symbols,
            env,
            errors,
        ),
        Expr::BinaryOp { left, op, right } => {
            infer_binary_operand_types_collecting(left, op, right, &spanned.span, symbols, env, errors);
            Some(TypeShape::Boolean)
        }
    }
}

fn validate_spanned_expr_list_collecting(
    expressions: &[SpannedExpr],
    symbols: &SymbolTable,
    env: &TypeEnv,
    errors: &mut Vec<CompilerError>,
) {
    for expression in expressions {
        infer_expr_type_collecting(expression, symbols, env, errors);
    }
}

fn infer_array_type_collecting(
    values: &[SpannedExpr],
    span: &Span,
    symbols: &SymbolTable,
    env: &TypeEnv,
    errors: &mut Vec<CompilerError>,
) -> Option<TypeShape> {
    let mut item_type = None;

    for value in values {
        if let Some(found) = infer_expr_type_collecting(value, symbols, env, errors) {
            if let Some(expected) = item_type.as_ref() {
                ensure_types_match_collecting(expected, &found, span, errors);
            } else {
                item_type = Some(found);
            }
        }
    }

    item_type.map(|shape| TypeShape::List(Box::new(shape)))
}

fn infer_execute_type_collecting(
    _agent_name: &str,
    kwargs: &[(String, SpannedExpr)],
    require_type: Option<&DataType>,
    _span: &Span,
    symbols: &SymbolTable,
    env: &TypeEnv,
    errors: &mut Vec<CompilerError>,
) -> Option<TypeShape> {
    for (_, value) in kwargs {
        infer_expr_type_collecting(value, symbols, env, errors);
    }

    require_type.and_then(|data_type| known_type_shape(data_type, symbols))
}

fn infer_binary_operand_types_collecting(
    left: &SpannedExpr,
    op: &BinaryOp,
    right: &SpannedExpr,
    span: &Span,
    symbols: &SymbolTable,
    env: &TypeEnv,
    errors: &mut Vec<CompilerError>,
) {
    let left_type = infer_expr_type_collecting(left, symbols, env, errors);
    let right_type = infer_expr_type_collecting(right, symbols, env, errors);

    if let (Some(left_type), Some(right_type)) = (left_type, right_type) {
        match op {
            BinaryOp::Equal | BinaryOp::NotEqual => {
                ensure_types_match_collecting(&left_type, &right_type, span, errors);
            }
            BinaryOp::LessThan
            | BinaryOp::GreaterThan
            | BinaryOp::LessEq
            | BinaryOp::GreaterEq => {
                if left_type.is_numeric() && right_type.is_numeric() {
                    return;
                }

                let found = if !left_type.is_numeric() {
                    left_type.display()
                } else {
                    right_type.display()
                };

                errors.push(CompilerError::TypeMismatch {
                    expected: "numeric".to_owned(),
                    found,
                    span: span.clone(),
                });
            }
        }
    }
}

fn ensure_types_match_collecting(
    expected: &TypeShape,
    found: &TypeShape,
    span: &Span,
    errors: &mut Vec<CompilerError>,
) {
    if expected != found {
        errors.push(CompilerError::TypeMismatch {
            expected: expected.display(),
            found: found.display(),
            span: span.clone(),
        });
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

fn known_type_shape(data_type: &DataType, symbols: &SymbolTable) -> Option<TypeShape> {
    match data_type {
        DataType::Custom(name, _) if !symbols.has_type(name) => None,
        DataType::List(inner, _) => {
            known_type_shape(inner, symbols).map(|inner| TypeShape::List(Box::new(inner)))
        }
        _ => Some(type_shape_from_data_type(data_type)),
    }
}

fn collect_error(errors: &mut Vec<CompilerError>, result: CompilerResult<()>) {
    if let Err(error) = result {
        errors.push(error);
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

    fn is_numeric(&self) -> bool {
        matches!(self, Self::Int | Self::Float)
    }
}
