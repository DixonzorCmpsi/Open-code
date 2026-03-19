use std::fs;
use tempfile::tempdir;
use crate::parser;
use crate::semantic;
use crate::codegen;

#[test]
fn test_codegen_opencode_json() {
    let input = r#"
type SearchResult { url: string }
tool WebSearch(query: string) -> SearchResult { invoke: module("a").function("b") }
client LocalQwen { provider = "local", model = "local.qwen2.5-coder:7b" }
agent Researcher { client = LocalQwen, tools = [WebSearch] }
workflow FindInfo(topic: string) -> SearchResult {
    let r = execute Researcher.run(task: "find ${topic}", require_type: SearchResult)
    return r
}
"#;
    let doc = parser::parse(input).expect("parse failed");
    semantic::analyze(&doc).expect("semantic failed");
    
    let dir = tempdir().unwrap();
    codegen::generate_opencode(&doc, dir.path()).expect("codegen failed");
    
    let config_path = dir.path().join("opencode.json");
    let content = fs::read_to_string(config_path).unwrap();
    let config: serde_json::Value = serde_json::from_str(&content).unwrap();
    
    assert_eq!(config["agents"]["coder"]["model"], "local.qwen2.5-coder:7b");
    assert_eq!(config["mcpServers"]["claw-tools"]["type"], "stdio");
    assert_eq!(config["mcpServers"]["claw-tools"]["command"], "node");
    assert_eq!(config["mcpServers"]["claw-tools"]["args"][0], "generated/mcp-server.js");
    assert!(config["contextPaths"].as_array().unwrap().contains(&serde_json::json!("generated/claw-context.md")));
}

#[test]
fn test_codegen_opencode_json_merge() {
    let input = r#"
client LocalQwen { provider = "local", model = "local.qwen2.5-coder:7b" }
agent Researcher { client = LocalQwen }
"#;
    let doc = parser::parse(input).expect("parse failed");
    
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("opencode.json");
    fs::write(&config_path, r#"
{
  "theme": "dark",
  "keybindings": { "submit": "ctrl+enter" },
  "agents": { "coder": { "model": "old-model", "temperature": 0.7 } }
}
"#).unwrap();

    codegen::generate_opencode(&doc, dir.path()).expect("codegen failed");
    
    let content = fs::read_to_string(config_path).unwrap();
    let config: serde_json::Value = serde_json::from_str(&content).unwrap();
    
    assert_eq!(config["theme"], "dark");
    assert_eq!(config["keybindings"]["submit"], "ctrl+enter");
    assert_eq!(config["agents"]["coder"]["model"], "local.qwen2.5-coder:7b");
    assert_eq!(config["agents"]["coder"]["temperature"], 0.7);
    assert_eq!(config["mcpServers"]["claw-tools"]["type"], "stdio");
}

#[test]
fn test_codegen_workflow_command_file() {
    let input = r#"
type SearchResult { url: string }
agent Researcher { system_prompt = "P" }
workflow FindInfo(topic: string) -> SearchResult {
    let r = execute Researcher.run(task: "find ${topic}", require_type: SearchResult)
    return r
}
"#;
    let doc = parser::parse(input).expect("parse failed");
    let dir = tempdir().unwrap();
    codegen::generate_opencode(&doc, dir.path()).expect("codegen failed");
    
    let cmd_path = dir.path().join(".opencode/commands/FindInfo.md");
    let content = fs::read_to_string(cmd_path).unwrap();
    
    assert!(content.contains("$TOPIC"));
    assert!(content.contains("agent_Researcher"));
    assert!(!content.contains("$topic"));
    assert!(!content.contains("$ARGUMENTS"));
}

#[test]
fn test_codegen_mcp_server_js() {
    let input = r#"
tool TestTool() {}
agent Researcher { system_prompt = "R" }
"#;
    let doc = parser::parse(input).expect("parse failed");
    let dir = tempdir().unwrap();
    codegen::generate_mcp(&doc, dir.path()).expect("codegen failed");
    
    let mcp_path = dir.path().join("generated/mcp-server.js");
    let content = fs::read_to_string(mcp_path).unwrap();
    
    assert!(content.contains("\"TestTool\""));
    assert!(content.contains("\"agent_Researcher\""));
    assert!(content.contains("type: \"object\""));
    assert!(content.contains("validateOutput"));
    assert!(content.contains("opencode -p"));
}

#[test]
fn test_codegen_context_document() {
    let input = r#"
type SearchResult { url: string }
client LocalQwen { provider = "local", model = "local.qwen2.5-coder:7b" }
agent Researcher { client = LocalQwen }
workflow FindInfo(topic: string) -> SearchResult { return execute Researcher.run(task: "t") }
"#;
    let doc = parser::parse(input).expect("parse failed");
    let dir = tempdir().unwrap();
    codegen::generate_opencode(&doc, dir.path()).expect("codegen failed");
    
    let ctx_path = dir.path().join("generated/claw-context.md");
    let content = fs::read_to_string(ctx_path).unwrap();
    
    assert!(content.contains("SearchResult"));
    assert!(content.contains("Researcher"));
    assert!(content.contains("FindInfo"));
    assert!(content.contains("local.qwen2.5-coder:7b"));
}

#[test]
fn test_codegen_no_agent_markdown_files() {
    let input = "agent A { system_prompt = 'S' }";
    let doc = parser::parse(input).expect("parse failed");
    let dir = tempdir().unwrap();
    codegen::generate_opencode(&doc, dir.path()).expect("codegen failed");
    
    let agents_dir = dir.path().join(".opencode/agents");
    assert!(!agents_dir.exists());
}
