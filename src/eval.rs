use std::collections::HashMap;

use crate::ast::{BinaryOp, Block, DataType, Document, ElseBranch, Expr, MockDecl, SpannedExpr, Statement};
use crate::errors::CompilerError;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Array(Vec<Value>),
    Object(HashMap<String, Value>),
    Null,
}

pub fn evaluate_tests(document: &Document) -> Result<(), CompilerError> {
    if document.tests.is_empty() {
        println!("No tests found.");
        return Ok(());
    }

    let mut success_count = 0;
    
    for test in &document.tests {
        println!("Running test: {}", test.name);
        let mut env = Environment::new();
        match eval_block(&test.body, &mut env, document) {
            Ok(Some(val)) => {
                println!("Test {} completed. Returned: {:?}", test.name, val);
                success_count += 1;
            }
            Ok(None) => {
                println!("Test {} passed.", test.name);
                success_count += 1;
            }
            Err(e) => {
                println!("Test {} failed: {}", test.name, e);
                return Err(e);
            }
        }
    }
    
    println!("{} tests passed successfully.", success_count);
    Ok(())
}

struct Environment {
    variables: HashMap<String, Value>,
}

impl Environment {
    fn new() -> Self {
        Self {
            variables: HashMap::new(),
        }
    }
    fn get(&self, name: &str) -> Option<&Value> {
        self.variables.get(name)
    }
    fn set(&mut self, name: String, value: Value) {
        self.variables.insert(name, value);
    }
}

fn eval_block(block: &Block, env: &mut Environment, doc: &Document) -> Result<Option<Value>, CompilerError> {
    for stmt in &block.statements {
        match stmt {
            Statement::LetDecl { name, value, .. } => {
                let evaled = eval_expr(value, env, doc)?;
                env.set(name.clone(), evaled);
            }
            Statement::Return { value, .. } => {
                return Ok(Some(eval_expr(value, env, doc)?));
            }
            Statement::Assert { condition, message, span } => {
                let evaled = eval_expr(condition, env, doc)?;
                if let Value::Bool(b) = evaled {
                    if !b {
                        let msg = message.clone().unwrap_or_else(|| "Assertion failed".to_string());
                        return Err(CompilerError::ParseError { 
                            message: format!("Test Assertion Failed: {}", msg),
                            span: span.clone(),
                        });
                    }
                } else {
                    return Err(CompilerError::ParseError { 
                        message: "Assert condition must be a boolean".to_string(),
                        span: span.clone(),
                    });
                }
            }
            Statement::Expression(expr) => {
                eval_expr(expr, env, doc)?;
            }
            _ => {
                // Ignore other statements for this simple AST interpreter
            }
        }
    }
    Ok(None)
}

fn eval_expr(expr: &SpannedExpr, env: &mut Environment, doc: &Document) -> Result<Value, CompilerError> {
    match &expr.expr {
        Expr::StringLiteral(s) => Ok(Value::String(s.clone())),
        Expr::IntLiteral(i) => Ok(Value::Int(*i)),
        Expr::FloatLiteral(f) => Ok(Value::Float(*f)),
        Expr::BoolLiteral(b) => Ok(Value::Bool(*b)),
        Expr::Identifier(name) => {
            if let Some(val) = env.get(name) {
                return Ok(val.clone());
            }
            if name.contains('.') {
                let parts: Vec<&str> = name.split('.').collect();
                if let Some(base_val) = env.get(parts[0]) {
                    let mut current = base_val.clone();
                    for field in &parts[1..] {
                        if let Value::Object(obj) = current {
                            current = obj.get(*field).cloned().ok_or_else(|| CompilerError::ParseError {
                                message: format!("Field '{}' not found", field),
                                span: expr.span.clone(),
                            })?;
                        } else {
                            return Err(CompilerError::ParseError {
                                message: format!("Cannot access field '{}' on non-object", field),
                                span: expr.span.clone(),
                            });
                        }
                    }
                    return Ok(current);
                }
            }
            Err(CompilerError::ParseError {
                message: format!("Undefined variable: {}", name),
                span: expr.span.clone(),
            })
        }
        Expr::Call(name, args) => {
            if name == "assert" {
                if let Some(arg) = args.get(0) {
                    let val = eval_expr(arg, env, doc)?;
                    if let Value::Bool(b) = val {
                        if !b {
                            return Err(CompilerError::ParseError {
                                message: "Assertion failed".to_string(),
                                span: expr.span.clone(),
                            });
                        }
                        return Ok(Value::Null);
                    } else {
                        return Err(CompilerError::ParseError {
                            message: "Assert must take a boolean".to_string(),
                            span: expr.span.clone(),
                        });
                    }
                }
            }

            if name == "print" {
                for arg in args {
                    let val = eval_expr(arg, env, doc)?;
                    match val {
                        Value::String(s) => print!("{} ", s),
                        Value::Int(i) => print!("{} ", i),
                        Value::Float(f) => print!("{} ", f),
                        Value::Bool(b) => print!("{} ", b),
                        Value::Object(o) => print!("{:?} ", o),
                        _ => print!("{:?} ", val),
                    }
                }
                println!();
                return Ok(Value::Null);
            }

            if name == "write_file" {
                if args.len() == 2 {
                    let path_val = eval_expr(&args[0], env, doc)?;
                    let content_val = eval_expr(&args[1], env, doc)?;
                    if let (Value::String(path), Value::String(content)) = (path_val, content_val) {
                        std::fs::write(&path, content).map_err(|e| CompilerError::ParseError {
                            message: format!("Failed to write file {}: {}", path, e),
                            span: expr.span.clone(),
                        })?;
                        return Ok(Value::Null);
                    }
                }
                return Err(CompilerError::ParseError {
                    message: "write_file requires two strings (path, content)".to_string(),
                    span: expr.span.clone(),
                });
            }

            // Check if it's a workflow call!
            if let Some(workflow) = doc.workflows.iter().find(|w| w.name == *name) {
                let mut local_env = Environment::new();
                for (i, arg_expr) in args.iter().enumerate() {
                    let val = eval_expr(arg_expr, env, doc)?;
                    if let Some(arg_decl) = workflow.arguments.get(i) {
                        local_env.set(arg_decl.name.clone(), val);
                    }
                }
                match eval_block(&workflow.body, &mut local_env, doc)? {
                    Some(val) => Ok(val),
                    None => Ok(Value::Null),
                }
            } else {
                Err(CompilerError::ParseError {
                    message: format!("Unknown function or workflow called: {}", name),
                    span: expr.span.clone(),
                })
            }
        }
        Expr::ExecuteRun { agent_name, kwargs, .. } => {
            // Find a MockDecl intercepting this execution!
            // We just look for ANY mock targeting this agent for simplicity.
            for mock in &doc.mocks {
                if mock.target_agent == *agent_name {
                    // Turn output into an Object
                    let mut obj = HashMap::new();
                    for (k, v_expr) in &mock.output {
                        obj.insert(k.clone(), eval_expr(v_expr, env, doc)?);
                    }
                    return Ok(Value::Object(obj));
                }
            }
            Err(CompilerError::ParseError {
                message: format!("No mock found for Agent '{}' in offline test execution!", agent_name),
                span: expr.span.clone(),
            })
        }
        Expr::MemberAccess(base, field) => {
            let base_val = eval_expr(base, env, doc)?;
            if let Value::Object(obj) = base_val {
                obj.get(field).cloned().ok_or_else(|| CompilerError::ParseError {
                    message: format!("Field '{}' not found on object", field),
                    span: expr.span.clone(),
                })
            } else {
                Err(CompilerError::ParseError {
                    message: format!("Cannot access field '{}' on non-object", field),
                    span: expr.span.clone(),
                })
            }
        }
        Expr::BinaryOp { left, op, right } => {
            let left_val = eval_expr(left, env, doc)?;
            let right_val = eval_expr(right, env, doc)?;
            match op {
                BinaryOp::Equal => Ok(Value::Bool(left_val == right_val)),
                BinaryOp::NotEqual => Ok(Value::Bool(left_val != right_val)),
                _ => Ok(Value::Bool(false)),
            }
        }
        _ => Ok(Value::Null)
    }
}
