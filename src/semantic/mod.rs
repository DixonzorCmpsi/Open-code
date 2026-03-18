mod symbols;
mod types;

use crate::ast::Document;
use crate::errors::CompilerResult;
use symbols::SymbolTable;
use types::{validate_references, validate_types};

pub struct Analyzer;

impl Analyzer {
    pub fn validate(document: &Document) -> CompilerResult<()> {
        let symbols = SymbolTable::build(document)?;
        validate_references(document, &symbols)?;
        validate_types(document, &symbols)
    }
}

pub fn analyze(document: &Document) -> CompilerResult<()> {
    Analyzer::validate(document)
}

#[cfg(test)]
mod tests {
    use super::Analyzer;
    use crate::ast::{
        AgentDecl, AgentSettings, Block, ClientDecl, DataType, Document, Expr, ImportDecl, Span,
        SpannedExpr, Statement, ToolDecl, TypeDecl, TypeField, WorkflowDecl,
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
            span: 121..150,
        });

        let error = Analyzer::validate(&document).unwrap_err();

        match error {
            CompilerError::DuplicateSymbol {
                name,
                first_span,
                second_span,
            } => {
                assert_eq!(name, "Researcher");
                assert_eq!(first_span, 40..58);
                assert_eq!(second_span, 121..150);
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
            source: "@openclaw/tools.browser".to_owned(),
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
                model: "gpt-5.4".to_owned(),
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
                span: 59..90,
            }],
            agents: vec![AgentDecl {
                name: "Researcher".to_owned(),
                extends: None,
                client: Some("FastOpenAI".to_owned()),
                system_prompt: Some("be precise".to_owned()),
                tools: vec!["WebSearch".to_owned()],
                settings: empty_settings(40, 58),
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
