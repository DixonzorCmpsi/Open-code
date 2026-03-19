use crate::parser;
use crate::ast::*;
use crate::errors::CompilerError;

#[test]
fn test_parse_full_document() {
    let input = r#"
type SearchResult {
    url: string
    snippet: string
    confidence_score: float
}

tool WebSearch(query: string) -> SearchResult {
    invoke: module("scripts/search").function("run")
}

client LocalQwen {
    provider = "local"
    model = "local.qwen2.5-coder:7b"
}

agent Researcher {
    client = LocalQwen
    system_prompt = "You are a precise researcher."
    tools = [WebSearch]
    settings = {
        max_steps: 5,
        temperature: 0.1
    }
}

workflow FindInfo(topic: string) -> SearchResult {
    let result: SearchResult = execute Researcher.run(
        task: "Find info about: ${topic}",
        require_type: SearchResult
    )
    return result
}
"#;

    let document = parser::parse(input).expect("parsing failed");

    assert_eq!(document.types.len(), 1);
    assert_eq!(document.types[0].name, "SearchResult");
    assert_eq!(document.types[0].fields.len(), 3);

    assert_eq!(document.tools.len(), 1);
    assert_eq!(document.tools[0].name, "WebSearch");

    assert_eq!(document.clients.len(), 1);
    assert_eq!(document.clients[0].model, "local.qwen2.5-coder:7b");

    assert_eq!(document.agents.len(), 1);
    assert_eq!(document.agents[0].name, "Researcher");
    assert_eq!(document.agents[0].tools, vec!["WebSearch".to_string()]);

    assert_eq!(document.workflows.len(), 1);
    assert_eq!(document.workflows[0].name, "FindInfo");
    assert_eq!(document.workflows[0].arguments.len(), 1);
    assert_eq!(document.workflows[0].arguments[0].name, "topic");
}

#[test]
fn test_parse_all_primitive_types() {
    let input = r#"
type AllTypes {
    a: string
    b: int
    c: float
    d: boolean
    e: list<string>
}
"#;
    let document = parser::parse(input).expect("parsing failed");
    let fields = &document.types[0].fields;

    assert!(matches!(fields[0].data_type, DataType::String(_)));
    assert!(matches!(fields[1].data_type, DataType::Int(_)));
    assert!(matches!(fields[2].data_type, DataType::Float(_)));
    assert!(matches!(fields[3].data_type, DataType::Boolean(_)));
    assert!(matches!(fields[4].data_type, DataType::List(..)));
}

#[test]
fn test_parse_string_interpolation() {
    let input = r#"
workflow Greet(name: string) -> string {
    let msg = "Hello ${name}"
    return msg
}
"#;
    let document = parser::parse(input).expect("parsing failed");
    let workflow = &document.workflows[0];
    if let Statement::LetDecl { value, .. } = &workflow.body.statements[0] {
        if let Expr::StringLiteral(s) = &value.expr {
            assert_eq!(s, "Hello ${name}");
        } else {
            panic!("Expected StringLiteral");
        }
    } else {
        panic!("Expected LetDecl");
    }
}

#[test]
fn test_parse_error_missing_brace() {
    let input = "type Foo { url: string";
    let res = parser::parse(input);
    assert!(matches!(res, Err(CompilerError::ParseError { .. })));
}

#[test]
fn test_parse_error_unknown_primitive() {
    let input = "type Foo { x: unknown }";
    let res = parser::parse(input);
    assert!(matches!(res, Err(CompilerError::ParseError { .. })));
}

#[test]
fn test_parse_error_empty_tools_list() {
    let input = "agent A { tools = [] }";
    let res = parser::parse(input);
    assert!(matches!(res, Err(CompilerError::ParseError { .. })));
}
