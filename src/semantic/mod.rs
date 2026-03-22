mod symbols;
mod types;

use crate::ast::{Block, DataType, Document, ElseBranch, Statement};
use crate::errors::CompilerResult;
use symbols::SymbolTable;
use types::{validate_references_collecting, validate_types_collecting};

pub struct Analyzer;

impl Analyzer {
    pub fn validate(document: &Document) -> CompilerResult<()> {
        analyze(document)
    }
}

pub fn analyze(document: &Document) -> CompilerResult<()> {
    analyze_collecting(document).into_result()
}

#[derive(Debug, Default)]
pub struct CompilationReport {
    pub errors: Vec<crate::errors::CompilerError>,
}

impl CompilationReport {
    pub fn into_result(self) -> CompilerResult<()> {
        if let Some(first) = self.errors.into_iter().next() {
            Err(first)
        } else {
            Ok(())
        }
    }
}

pub fn analyze_collecting(document: &Document) -> CompilationReport {
    const MAX_ERRORS: usize = 50;

    let mut errors = Vec::new();

    match SymbolTable::build(document) {
        Ok(symbols) => {
            errors.extend(detect_circular_types(document));
            errors.extend(detect_circular_agents(document));
            errors.extend(validate_references_collecting(document, &symbols));

            if errors.len() < MAX_ERRORS {
                errors.extend(validate_types_collecting(document, &symbols));
            }

            if errors.len() < MAX_ERRORS {
                errors.extend(check_exhaustive_returns(document));
            }
        }
        Err(error) => errors.push(error),
    }

    errors.sort_by_key(|error| error.span().map_or(usize::MAX, |span| span.start));
    errors.truncate(MAX_ERRORS);

    CompilationReport { errors }
}

fn detect_circular_types(document: &Document) -> Vec<crate::errors::CompilerError> {
    let mut errors = Vec::new();

    for declaration in &document.types {
        let mut path = Vec::new();
        check_type_cycle(document, &declaration.name, &mut path, &declaration.span, &mut errors);
    }

    errors
}

fn check_type_cycle(
    document: &Document,
    type_name: &str,
    path: &mut Vec<String>,
    origin_span: &crate::ast::Span,
    errors: &mut Vec<crate::errors::CompilerError>,
) {
    if let Some(index) = path.iter().position(|item| item == type_name) {
        let mut cycle_path = path[index..].to_vec();
        cycle_path.push(type_name.to_owned());
        errors.push(crate::errors::CompilerError::CircularType {
            type_name: type_name.to_owned(),
            cycle_path,
            span: origin_span.clone(),
        });
        return;
    }

    path.push(type_name.to_owned());

    if let Some(declaration) = document.types.iter().find(|item| item.name == type_name) {
        for field in &declaration.fields {
            check_data_type_cycle(document, &field.data_type, path, origin_span, errors);
        }
    }

    path.pop();
}

fn check_data_type_cycle(
    document: &Document,
    data_type: &DataType,
    path: &mut Vec<String>,
    origin_span: &crate::ast::Span,
    errors: &mut Vec<crate::errors::CompilerError>,
) {
    match data_type {
        DataType::Custom(name, _) => check_type_cycle(document, name, path, origin_span, errors),
        DataType::List(inner, _) => check_data_type_cycle(document, inner, path, origin_span, errors),
        DataType::String(_) | DataType::Int(_) | DataType::Float(_) | DataType::Boolean(_) => {}
    }
}

fn detect_circular_agents(document: &Document) -> Vec<crate::errors::CompilerError> {
    let mut errors = Vec::new();

    for declaration in &document.agents {
        let mut path = Vec::new();
        check_agent_extends_cycle(document, &declaration.name, &mut path, &declaration.span, &mut errors);
    }

    errors
}

fn check_agent_extends_cycle(
    document: &Document,
    agent_name: &str,
    path: &mut Vec<String>,
    origin_span: &crate::ast::Span,
    errors: &mut Vec<crate::errors::CompilerError>,
) {
    if let Some(index) = path.iter().position(|item| item == agent_name) {
        let mut cycle_path = path[index..].to_vec();
        cycle_path.push(agent_name.to_owned());
        errors.push(crate::errors::CompilerError::CyclicDependency {
            message: format!("circular agent extentions detected: {}", cycle_path.join(" -> ")),
            span: origin_span.clone(),
        });
        return;
    }

    path.push(agent_name.to_owned());

    if let Some(declaration) = document.agents.iter().find(|item| item.name == agent_name) {
        if let Some(extends) = &declaration.extends {
            check_agent_extends_cycle(document, extends, path, origin_span, errors);
        }
    }

    path.pop();
}

fn check_exhaustive_returns(document: &Document) -> Vec<crate::errors::CompilerError> {
    document
        .workflows
        .iter()
        .filter(|workflow| workflow.return_type.is_some() && !block_always_returns(&workflow.body))
        .map(|workflow| crate::errors::CompilerError::MissingReturn {
            workflow_name: workflow.name.clone(),
            span: workflow.span.clone(),
        })
        .collect()
}

fn block_always_returns(block: &Block) -> bool {
    block.statements.iter().any(statement_always_returns)
}

fn statement_always_returns(statement: &Statement) -> bool {
    match statement {
        Statement::Return { .. } => true,
        Statement::IfCond { if_body, else_body, .. } => {
            block_always_returns(if_body)
                && else_body
                    .as_ref()
                    .is_some_and(else_branch_always_returns)
        }
        Statement::TryCatch {
            try_body,
            catch_body,
            ..
        } => block_always_returns(try_body) && block_always_returns(catch_body),
        Statement::LetDecl { .. }
        | Statement::ForLoop { .. }
        | Statement::ExecuteRun { .. }
        | Statement::Assert { .. }
        | Statement::Continue(_)
        | Statement::Break(_)
        | Statement::Reason { .. }
        | Statement::Expression(_) => false,
    }
}

fn else_branch_always_returns(else_branch: &ElseBranch) -> bool {
    match else_branch {
        ElseBranch::Else(block) => block_always_returns(block),
        ElseBranch::ElseIf(statement) => statement_always_returns(statement),
    }
}

#[cfg(test)]
mod tests {
    use super::{analyze, analyze_collecting, Analyzer, CompilationReport};
    use crate::ast::{
        AgentDecl, AgentSettings, BinaryOp, Block, ClientDecl, DataType, Document, Expr,
        ImportDecl, Span, SpannedExpr, Statement, ToolDecl, TypeDecl, TypeField, WorkflowDecl,
    };
    use crate::errors::CompilerError;

    fn spanned(expr: Expr, span: Span) -> SpannedExpr {
        SpannedExpr { expr, span }
    }

    #[test]
    fn validates_a_well_formed_document() {
        let document = valid_document();

        assert!(Analyzer::validate(&document).is_ok());
    }

    #[test]
    fn rejects_duplicate_symbols() {
        let mut document = valid_document();
        document.agents.push(AgentDecl {
            name: "Researcher".to_owned(),
            extends: None,
            client: None,
            system_prompt: None,
            tools: Vec::new(),
            settings: empty_settings(121, 123),
            dynamic_reasoning: std::cell::Cell::new(false),
            span: 121..150,
        });

        let error = Analyzer::validate(&document).unwrap_err();

        match error {
            CompilerError::DuplicateDeclaration {
                message,
                span,
            } => {
                assert!(message.contains("Researcher"));
                assert_eq!(span, 121..150);
            }
            other => panic!("expected duplicate symbol error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_missing_tool_reference_in_agent() {
        let mut document = valid_document();
        document.agents[0].tools.push("MissingTool".to_owned());

        let error = Analyzer::validate(&document).unwrap_err();

        match error {
            CompilerError::UndefinedTool { name, span } => {
                assert_eq!(name, "MissingTool");
                assert_eq!(span, 40..58);
            }
            other => panic!("expected undefined tool error, got {other:?}"),
        }
    }

    #[test]
    fn accepts_imported_tools_as_valid_agent_dependencies() {
        let mut document = valid_document();
        document.imports.push(ImportDecl {
            names: vec!["ImportedTool".to_owned()],
            source: "@claw/tools.browser".to_owned(),
            span: 0..22,
        });
        document.agents[0].tools.push("ImportedTool".to_owned());

        assert!(Analyzer::validate(&document).is_ok());
    }

    #[test]
    fn rejects_missing_extended_agent() {
        let mut document = valid_document();
        document.agents[0].extends = Some("BaseAgent".to_owned());

        let error = Analyzer::validate(&document).unwrap_err();

        match error {
            CompilerError::UndefinedAgent { name, span } => {
                assert_eq!(name, "BaseAgent");
                assert_eq!(span, 40..58);
            }
            other => panic!("expected undefined agent error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_missing_client_reference() {
        let mut document = valid_document();
        document.agents[0].client = Some("MissingClient".to_owned());

        let error = Analyzer::validate(&document).unwrap_err();

        match error {
            CompilerError::UndefinedClient { name, span } => {
                assert_eq!(name, "MissingClient");
                assert_eq!(span, 40..58);
            }
            other => panic!("expected undefined client error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_missing_execute_agent() {
        let mut document = valid_document();
        document.workflows[0].return_type = None;
        document.workflows[0].body.statements = vec![Statement::LetDecl {
            name: "result".to_owned(),
            explicit_type: Some(custom_type("SearchResult", 200, 212)),
            value: spanned(
                Expr::ExecuteRun {
                    agent_name: "MissingAgent".to_owned(),
                    kwargs: vec![("task".to_owned(), spanned(Expr::StringLiteral("find".to_owned()), 230..236))],
                    require_type: Some(custom_type("SearchResult", 245, 257)),
                },
                180..258,
            ),
            span: 180..258,
        }];

        let error = Analyzer::validate(&document).unwrap_err();

        match error {
            CompilerError::UndefinedAgent { name, span } => {
                assert_eq!(name, "MissingAgent");
                assert_eq!(span, 180..258);
            }
            other => panic!("expected undefined agent error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_unknown_execute_required_type() {
        let mut document = valid_document();
        document.workflows[0].return_type = None;
        document.workflows[0].body.statements = vec![Statement::LetDecl {
            name: "result".to_owned(),
            explicit_type: Some(custom_type("SearchResult", 200, 212)),
            value: spanned(
                Expr::ExecuteRun {
                    agent_name: "Researcher".to_owned(),
                    kwargs: vec![("task".to_owned(), spanned(Expr::StringLiteral("find".to_owned()), 230..236))],
                    require_type: Some(custom_type("MissingType", 245, 256)),
                },
                180..257,
            ),
            span: 180..257,
        }];

        let error = Analyzer::validate(&document).unwrap_err();

        match error {
            CompilerError::UndefinedType { name, span } => {
                assert_eq!(name, "MissingType");
                assert_eq!(span, 245..256);
            }
            other => panic!("expected undefined type error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_execute_assignment_type_mismatches() {
        let mut document = valid_document();
        document.workflows[0].return_type = None;
        document.workflows[0].body.statements = vec![Statement::LetDecl {
            name: "result".to_owned(),
            explicit_type: Some(DataType::String(200..206)),
            value: spanned(
                Expr::ExecuteRun {
                    agent_name: "Researcher".to_owned(),
                    kwargs: vec![("task".to_owned(), spanned(Expr::StringLiteral("find".to_owned()), 230..236))],
                    require_type: Some(custom_type("SearchResult", 245, 257)),
                },
                180..258,
            ),
            span: 180..258,
        }];

        let error = Analyzer::validate(&document).unwrap_err();

        match error {
            CompilerError::TypeMismatch {
                expected,
                found,
                span,
            } => {
                assert_eq!(expected, "string");
                assert_eq!(found, "SearchResult");
                assert_eq!(span, 180..258);
            }
            other => panic!("expected type mismatch error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_return_type_mismatches() {
        let mut document = valid_document();
        document.workflows[0].return_type = Some(DataType::Int(160..163));
        document.workflows[0].body.statements = vec![Statement::Return {
            value: spanned(Expr::StringLiteral("oops".to_owned()), 180..192),
            span: 180..192,
        }];

        let error = Analyzer::validate(&document).unwrap_err();

        match error {
            CompilerError::TypeMismatch {
                expected,
                found,
                span,
            } => {
                assert_eq!(expected, "int");
                assert_eq!(found, "string");
                assert_eq!(span, 180..192);
            }
            other => panic!("expected type mismatch error, got {other:?}"),
        }
    }

    #[test]
    fn accepts_builtin_error_types_in_catch_clauses() {
        let mut document = valid_document();
        document.workflows[0].return_type = None;
        document.workflows[0].body.statements = vec![Statement::TryCatch {
            try_body: Block {
                statements: vec![],
                span: 180..180,
            },
            catch_name: "error".to_owned(),
            catch_type: custom_type("AgentExecutionError", 198, 217),
            catch_body: Block {
                statements: vec![],
                span: 218..218,
            },
            span: 175..218,
        }];

        assert!(Analyzer::validate(&document).is_ok());
    }

    #[test]
    fn binds_catch_name_inside_catch_body() {
        let mut document = valid_document();
        document.workflows[0].return_type = None;
        document.workflows[0].body.statements = vec![Statement::TryCatch {
            try_body: Block {
                statements: vec![],
                span: 180..180,
            },
            catch_name: "error".to_owned(),
            catch_type: custom_type("ToolExecutionError", 198, 216),
            catch_body: Block {
                statements: vec![Statement::LetDecl {
                    name: "captured".to_owned(),
                    explicit_type: Some(custom_type("ToolExecutionError", 228, 246)),
                    value: spanned(Expr::Identifier("error".to_owned()), 249..254),
                    span: 218..254,
                }],
                span: 218..254,
            },
            span: 175..254,
        }];

        assert!(Analyzer::validate(&document).is_ok());
    }

    #[test]
    fn rejects_continue_outside_loops() {
        let mut document = valid_document();
        document.workflows[0].return_type = None;
        document.workflows[0].body.statements = vec![Statement::Continue(180..188)];

        let error = Analyzer::validate(&document).unwrap_err();

        match error {
            CompilerError::InvalidControlFlow { keyword, span } => {
                assert_eq!(keyword, "continue");
                assert_eq!(span, 180..188);
            }
            other => panic!("expected invalid control flow error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_break_outside_loops() {
        let mut document = valid_document();
        document.workflows[0].return_type = None;
        document.workflows[0].body.statements = vec![Statement::Break(180..185)];

        let error = Analyzer::validate(&document).unwrap_err();

        match error {
            CompilerError::InvalidControlFlow { keyword, span } => {
                assert_eq!(keyword, "break");
                assert_eq!(span, 180..185);
            }
            other => panic!("expected invalid control flow error, got {other:?}"),
        }
    }

    #[test]
    fn accepts_continue_inside_nested_if_within_loop() {
        let mut document = valid_document();
        document.workflows[0].return_type = None;
        document.workflows[0].arguments.push(TypeField {
            name: "items".to_owned(),
            data_type: DataType::List(Box::new(DataType::String(130..136)), 125..137),
            constraints: Vec::new(),
            span: 120..137,
        });
        document.workflows[0].body.statements = vec![Statement::ForLoop {
            item_name: "item".to_owned(),
            iterator: spanned(Expr::Identifier("items".to_owned()), 180..185),
            body: Block {
                statements: vec![Statement::IfCond {
                    condition: spanned(Expr::BoolLiteral(true), 200..204),
                    if_body: Block {
                        statements: vec![Statement::Continue(205..213)],
                        span: 205..213,
                    },
                    else_body: None,
                    span: 190..214,
                }],
                span: 186..214,
            },
            span: 180..214,
        }];

        assert!(Analyzer::validate(&document).is_ok());
    }

    #[test]
    fn rejects_ordering_comparisons_on_strings() {
        let mut document = valid_document();
        document.workflows[0].return_type = Some(DataType::Boolean(160..167));
        document.workflows[0].body.statements = vec![Statement::Return {
            value: spanned(
                Expr::BinaryOp {
                    left: Box::new(spanned(Expr::StringLiteral("a".to_owned()), 180..183)),
                    op: BinaryOp::LessThan,
                    right: Box::new(spanned(Expr::StringLiteral("b".to_owned()), 186..189)),
                },
                180..189,
            ),
            span: 180..189,
        }];

        let error = Analyzer::validate(&document).unwrap_err();

        match error {
            CompilerError::TypeMismatch {
                expected,
                found,
                span,
            } => {
                assert_eq!(expected, "numeric");
                assert_eq!(found, "string");
                assert_eq!(span, 180..189);
            }
            other => panic!("expected numeric ordering type mismatch, got {other:?}"),
        }
    }

    #[test]
    fn rejects_assert_outside_test_blocks() {
        let mut document = valid_document();
        document.workflows[0].return_type = None;
        document.workflows[0].body.statements = vec![Statement::Assert {
            condition: spanned(Expr::BoolLiteral(true), 180..184),
            message: Some("must stay in tests".to_owned()),
            span: 175..205,
        }];

        let error = Analyzer::validate(&document).unwrap_err();

        match error {
            CompilerError::InvalidAssertOutsideTest { span } => {
                assert_eq!(span, 175..205);
            }
            other => panic!("expected invalid assert error, got {other:?}"),
        }
    }

    #[test]
    fn accepts_assert_inside_test_blocks() {
        let mut document = valid_document();
        document.workflows.clear();
        document.tests.push(crate::ast::TestDecl {
            name: "guards invariants".to_owned(),
            body: Block {
                statements: vec![Statement::Assert {
                    condition: spanned(Expr::BoolLiteral(true), 180..184),
                    message: None,
                    span: 175..184,
                }],
                span: 175..184,
            },
            span: 170..185,
        });

        assert!(Analyzer::validate(&document).is_ok());
    }

    #[test]
    fn rejects_circular_type_references() {
        let mut document = valid_document();
        document.types = vec![
            TypeDecl {
                name: "A".to_owned(),
                fields: vec![TypeField {
                    name: "b".to_owned(),
                    data_type: custom_type("B", 10, 11),
                    constraints: Vec::new(),
                    span: 8..11,
                }],
                span: 0..12,
            },
            TypeDecl {
                name: "B".to_owned(),
                fields: vec![TypeField {
                    name: "a".to_owned(),
                    data_type: custom_type("A", 20, 21),
                    constraints: Vec::new(),
                    span: 18..21,
                }],
                span: 12..22,
            },
        ];

        let error = Analyzer::validate(&document).unwrap_err();

        match error {
            CompilerError::CircularType {
                type_name,
                cycle_path,
                span,
            } => {
                assert_eq!(type_name, "A");
                assert_eq!(cycle_path, vec!["A".to_owned(), "B".to_owned(), "A".to_owned()]);
                assert_eq!(span, 0..12);
            }
            other => panic!("expected circular type error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_missing_returns_on_all_paths() {
        let mut document = valid_document();
        document.workflows[0].body.statements = vec![Statement::IfCond {
            condition: spanned(Expr::BoolLiteral(true), 180..184),
            if_body: Block {
                statements: vec![Statement::Return {
                    value: spanned(Expr::Identifier("topic".to_owned()), 190..195),
                    span: 190..195,
                }],
                span: 185..196,
            },
            else_body: None,
            span: 180..196,
        }];

        let error = Analyzer::validate(&document).unwrap_err();

        match error {
            CompilerError::MissingReturn {
                workflow_name,
                span,
            } => {
                assert_eq!(workflow_name, "ResearchTopic");
                assert_eq!(span, 91..273);
            }
            other => panic!("expected missing return error, got {other:?}"),
        }
    }

    #[test]
    fn analyze_collecting_reports_multiple_errors() {
        let mut document = valid_document();
        document.workflows[0].return_type = None;
        document.agents[0].tools.push("MissingTool".to_owned());
        document.workflows[0].body.statements = vec![
            Statement::Assert {
                condition: spanned(Expr::BoolLiteral(true), 180..184),
                message: None,
                span: 175..184,
            },
            Statement::Continue(190..198),
        ];

        let report = analyze_collecting(&document);

        assert_eq!(report.errors.len(), 3);
        assert!(matches!(
            report.errors[0],
            CompilerError::UndefinedTool { .. }
        ));
        assert!(matches!(
            report.errors[1],
            CompilerError::InvalidAssertOutsideTest { .. }
        ));
        assert!(matches!(
            report.errors[2],
            CompilerError::InvalidControlFlow { .. }
        ));
    }

    #[test]
    fn analyze_delegates_to_collecting_results() {
        let report = CompilationReport {
            errors: vec![CompilerError::MissingReturn {
                workflow_name: "ResearchTopic".to_owned(),
                span: 91..273,
            }],
        };

        let error = report.into_result().unwrap_err();

        match error {
            CompilerError::MissingReturn {
                workflow_name,
                span,
            } => {
                assert_eq!(workflow_name, "ResearchTopic");
                assert_eq!(span, 91..273);
            }
            other => panic!("expected missing return from report, got {other:?}"),
        }

        let mut document = valid_document();
        document.workflows[0].return_type = None;
        document.workflows[0].body.statements = vec![Statement::Continue(180..188)];

        assert!(matches!(
            analyze(&document),
            Err(CompilerError::InvalidControlFlow { .. })
        ));
    }

    fn valid_document() -> Document {
        Document {
            imports: Vec::new(),
            types: vec![TypeDecl {
                name: "SearchResult".to_owned(),
                fields: vec![TypeField {
                    name: "title".to_owned(),
                    data_type: DataType::String(12..18),
                    constraints: Vec::new(),
                    span: 12..25,
                }],
                span: 0..30,
            }],
            clients: vec![ClientDecl {
                name: "FastOpenAI".to_owned(),
                provider: "openai".to_owned(),
                model: "gpt-5.1".to_owned(),
                retries: Some(2),
                timeout_ms: Some(5_000),
                endpoint: None,
                api_key: None,
                span: 31..39,
            }],
            tools: vec![ToolDecl {
                name: "WebSearch".to_owned(),
                arguments: vec![TypeField {
                    name: "query".to_owned(),
                    data_type: DataType::String(60..66),
                    constraints: Vec::new(),
                    span: 60..72,
                }],
                return_type: Some(custom_type("SearchResult", 73, 85)),
                invoke_path: Some("scripts.search.run".to_owned()),
                using: None,
                synthesizer: None,
                test_block: None,
                secrets: vec![],
                span: 59..90,
            }],
            agents: vec![AgentDecl {
                name: "Researcher".to_owned(),
                extends: None,
                client: Some("FastOpenAI".to_owned()),
                system_prompt: Some("be precise".to_owned()),
                tools: vec!["WebSearch".to_owned()],
                settings: empty_settings(40, 58),
                dynamic_reasoning: std::cell::Cell::new(false),
                span: 40..58,
            }],
            workflows: vec![WorkflowDecl {
                name: "ResearchTopic".to_owned(),
                arguments: vec![TypeField {
                    name: "topic".to_owned(),
                    data_type: DataType::String(100..106),
                    constraints: Vec::new(),
                    span: 100..112,
                }],
                return_type: Some(custom_type("SearchResult", 160, 172)),
                artifact: None,
                body: Block {
                    statements: vec![
                        Statement::LetDecl {
                            name: "result".to_owned(),
                            explicit_type: Some(custom_type("SearchResult", 200, 212)),
                            value: spanned(
                                Expr::ExecuteRun {
                                    agent_name: "Researcher".to_owned(),
                                    kwargs: vec![(
                                        "task".to_owned(),
                                        spanned(
                                            Expr::StringLiteral("find".to_owned()),
                                            230..236,
                                        ),
                                    )],
                                    require_type: Some(custom_type("SearchResult", 245, 257)),
                                },
                                180..258,
                            ),
                            span: 180..258,
                        },
                        Statement::Return {
                            value: spanned(
                                Expr::Identifier("result".to_owned()),
                                259..272,
                            ),
                            span: 259..272,
                        },
                    ],
                    span: 175..273,
                },
                span: 91..273,
            }],
            listeners: Vec::new(),
            tests: Vec::new(),
            mocks: Vec::new(),
            synthesizers: Vec::new(),
            span: 0..273,
        }
    }

    fn empty_settings(start: usize, end: usize) -> AgentSettings {
        AgentSettings {
            entries: Vec::new(),
            span: start..end,
        }
    }

    fn custom_type(name: &str, start: usize, end: usize) -> DataType {
        DataType::Custom(name.to_owned(), start..end)
    }
}
