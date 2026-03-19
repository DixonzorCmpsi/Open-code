use crate::parser;
use crate::semantic;
use crate::errors::CompilerError;

#[test]
fn test_semantic_valid_document() {
    let input = r#"
type SearchResult { url: string }
tool WebSearch(query: string) -> SearchResult { invoke: module("a").function("b") }
client LocalQwen { provider = "local", model = "m" }
agent Researcher { client = LocalQwen, tools = [WebSearch] }
workflow FindInfo(topic: string) -> SearchResult {
    let r: SearchResult = execute Researcher.run(task: "t", require_type: SearchResult)
    return r
}
"#;
    let doc = parser::parse(input).expect("parse failed");
    semantic::analyze(&doc).expect("semantic failed");
}

#[test]
fn test_semantic_undefined_tool() {
    let input = "agent A { tools = [MissingTool] }";
    let doc = parser::parse(input).expect("parse failed");
    let res = semantic::analyze(&doc);
    assert!(matches!(res, Err(CompilerError::UndefinedTool { .. })));
}

#[test]
fn test_semantic_undefined_agent() {
    let input = "workflow W() { execute GhostAgent.run(task: 't') }";
    let doc = parser::parse(input).expect("parse failed");
    let res = semantic::analyze(&doc);
    assert!(matches!(res, Err(CompilerError::UndefinedAgent { .. })));
}

#[test]
fn test_semantic_undefined_client() {
    let input = "agent A { client = MissingClient }";
    let doc = parser::parse(input).expect("parse failed");
    let res = semantic::analyze(&doc);
    assert!(matches!(res, Err(CompilerError::UndefinedClient { .. })));
}

#[test]
fn test_semantic_duplicate_type() {
    let input = "type Foo {} type Foo {}";
    let doc = parser::parse(input).expect("parse failed");
    let res = semantic::analyze(&doc);
    assert!(matches!(res, Err(CompilerError::DuplicateDeclaration { .. })));
}

#[test]
#[ignore]
fn test_semantic_type_mismatch_skipped() {
    // TODO: type mismatch not yet enforced
}
