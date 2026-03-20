use std::fs;
use std::process::Command;
use tempfile::tempdir;
use crate::parser;
use crate::semantic;
use crate::codegen;

fn find_node() -> Option<String> {
    let candidates = [
        "/opt/homebrew/bin/node",
        "/usr/local/bin/node",
        "/usr/bin/node",
        "node",
    ];
    for path in &candidates {
        if Command::new(path).arg("--version").output().is_ok() {
            return Some(path.to_string());
        }
    }
    None
}

// Minimal valid .claw document used across multiple tests
const FULL_DOC: &str = r#"
type SearchResult {
    url: string
    snippet: string
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
    system_prompt = "You are a researcher."
    tools = [WebSearch]
}

workflow FindInfo(topic: string) -> SearchResult {
    let r: SearchResult = execute Researcher.run(task: "find ${topic}", require_type: SearchResult)
    return r
}
"#;

#[test]
fn test_codegen_opencode_json() {
    let doc = parser::parse(FULL_DOC).expect("parse failed");
    semantic::analyze(&doc).expect("semantic failed");

    let dir = tempdir().unwrap();
    codegen::generate_opencode(&doc, dir.path()).expect("codegen failed");

    let content = fs::read_to_string(dir.path().join("opencode.json")).unwrap();
    let config: serde_json::Value = serde_json::from_str(&content).unwrap();

    // OpenCode 1.x schema: model at top level as "ollama/<id>"
    assert_eq!(config["model"], "ollama/qwen2.5-coder:7b");
    // provider.ollama block for local models
    assert_eq!(config["provider"]["ollama"]["api"], "http://localhost:11434/v1");
    assert!(config["provider"]["ollama"]["models"]["qwen2.5-coder:7b"].is_object());
    // mcp (not mcpServers), type=local (not stdio), command is an array
    assert_eq!(config["mcp"]["claw-tools"]["type"], "local");
    assert!(config["mcp"]["claw-tools"]["command"].is_array());
    // instructions (not contextPaths)
    let instructions = config["instructions"].as_array().unwrap();
    assert!(instructions.iter().any(|v| v == "generated/claw-context.md"));

    // Confirm wrong keys are NOT present (regression guard)
    assert!(config["mcpServers"].is_null());
    assert!(config["contextPaths"].is_null());
    assert!(config["agents"].is_null());
}

#[test]
fn test_codegen_opencode_json_cloud_model() {
    let input = r#"
client MyClaude {
    provider = "anthropic"
    model = "claude-4-sonnet"
}
agent Writer {
    client = MyClaude
    system_prompt = "Write."
}
"#;
    let doc = parser::parse(input).expect("parse failed");
    let dir = tempdir().unwrap();
    codegen::generate_opencode(&doc, dir.path()).expect("codegen failed");

    let content = fs::read_to_string(dir.path().join("opencode.json")).unwrap();
    let config: serde_json::Value = serde_json::from_str(&content).unwrap();

    // Cloud model: set at top-level model field directly
    assert_eq!(config["model"], "claude-4-sonnet");
    // No provider block for cloud models
    assert!(config["provider"].is_null());
    // No stale agents block
    assert!(config["agents"].is_null());
}

#[test]
fn test_codegen_opencode_json_merge() {
    let input = r#"
client LocalQwen {
    provider = "local"
    model = "local.qwen2.5-coder:7b"
}
agent Researcher {
    client = LocalQwen
    system_prompt = "Research."
}
"#;
    let doc = parser::parse(input).expect("parse failed");

    let dir = tempdir().unwrap();
    let config_path = dir.path().join("opencode.json");
    // Pre-write user-owned keys + an old managed key
    fs::write(&config_path, r#"{
  "theme": "dark",
  "keybindings": { "submit": "ctrl+enter" },
  "model": "old-model"
}"#).unwrap();

    codegen::generate_opencode(&doc, dir.path()).expect("codegen failed");

    let content = fs::read_to_string(config_path).unwrap();
    let config: serde_json::Value = serde_json::from_str(&content).unwrap();

    // User-owned keys preserved
    assert_eq!(config["theme"], "dark");
    assert_eq!(config["keybindings"]["submit"], "ctrl+enter");
    // Managed keys overwritten with correct OpenCode 1.x schema
    assert_eq!(config["model"], "ollama/qwen2.5-coder:7b");
    assert!(config["agents"].is_null(), "stale agents key should be removed");
    assert_eq!(config["mcp"]["claw-tools"]["type"], "local");
    assert!(config["mcpServers"].is_null(), "stale mcpServers key should be removed");
}

#[test]
fn test_codegen_workflow_command_file() {
    let input = r#"
type SearchResult { url: string }
agent Researcher {
    system_prompt = "Research carefully."
}
workflow FindInfo(topic: string) -> SearchResult {
    let r: SearchResult = execute Researcher.run(task: "find ${topic}", require_type: SearchResult)
    return r
}
"#;
    let doc = parser::parse(input).expect("parse failed");
    let dir = tempdir().unwrap();
    codegen::generate_opencode(&doc, dir.path()).expect("codegen failed");

    // v1.2.27 uses .opencode/command/ (singular)
    let cmd_path = dir.path().join(".opencode/command/FindInfo.md");
    let content = fs::read_to_string(&cmd_path)
        .unwrap_or_else(|_| panic!("command file not found at: {}", cmd_path.display()));

    assert!(content.contains("$TOPIC"), "should contain $TOPIC");
    assert!(content.contains("agent_Researcher"), "should contain agent_Researcher");
    assert!(!content.contains("$topic"), "should NOT contain $topic (lowercase)");
    assert!(!content.contains("$ARGUMENTS"), "should NOT contain $ARGUMENTS");
}

#[test]
fn test_codegen_mcp_server_js() {
    let input = r#"
tool TestTool(query: string) {
    invoke: module("scripts/test").function("run")
}
agent Researcher {
    system_prompt = "Research."
}
"#;
    let doc = parser::parse(input).expect("parse failed");
    let dir = tempdir().unwrap();
    // generate_mcp writes into generated/ which must already exist
    fs::create_dir_all(dir.path().join("generated")).unwrap();
    codegen::generate_mcp(&doc, dir.path()).expect("codegen failed");

    let content = fs::read_to_string(dir.path().join("generated/mcp-server.js")).unwrap();

    assert!(content.contains("\"TestTool\""), "should define TestTool");
    assert!(content.contains("\"agent_Researcher\""), "should define agent_Researcher");
    assert!(content.contains("type: \"object\""), "should include JSON Schema");
    assert!(content.contains("validateOutput"), "should include output validation");
    // Agent handler MUST call LLM API directly — never spawn a child opencode process
    assert!(!content.contains("opencode -p"), "agent handler must NOT spawn child opencode process");
    assert!(!content.contains("execSync"), "agent handler must NOT use execSync");
    // Default provider (no client declared) falls back to Anthropic
    assert!(content.contains("@anthropic-ai/sdk"), "agent handler should use Anthropic SDK by default");
    assert!(content.contains("while (steps <"), "agent handler should have a tool-call loop");
}

#[test]
fn test_codegen_context_document() {
    let input = r#"
type SearchResult { url: string }
client LocalQwen {
    provider = "local"
    model = "local.qwen2.5-coder:7b"
}
agent Researcher {
    client = LocalQwen
}
workflow FindInfo(topic: string) -> SearchResult {
    return execute Researcher.run(task: "t")
}
"#;
    let doc = parser::parse(input).expect("parse failed");
    let dir = tempdir().unwrap();
    codegen::generate_opencode(&doc, dir.path()).expect("codegen failed");

    let content = fs::read_to_string(dir.path().join("generated/claw-context.md")).unwrap();

    assert!(content.contains("SearchResult"), "should list types");
    assert!(content.contains("Researcher"), "should list agents");
    assert!(content.contains("FindInfo"), "should list workflows");
    // Context doc references client by name (not model string)
    assert!(content.contains("LocalQwen"), "should reference client name");
}

#[test]
fn test_codegen_no_agent_markdown_files() {
    let input = r#"
agent A {
    system_prompt = "Help."
}
"#;
    let doc = parser::parse(input).expect("parse failed");
    let dir = tempdir().unwrap();
    codegen::generate_opencode(&doc, dir.path()).expect("codegen failed");

    // Agents are MCP runner tools — no .opencode/agents/ directory should exist
    assert!(!dir.path().join(".opencode/agents").exists());
}

#[test]
fn test_codegen_no_baml_files_for_module_tools() {
    let doc = parser::parse(FULL_DOC).expect("parse failed");
    let dir = tempdir().unwrap();

    let baml_output = codegen::generate_baml(&doc).expect("baml codegen failed");
    // No baml(...) tools in FULL_DOC — functions should be empty
    assert!(baml_output.functions.is_empty(), "should produce no BAML functions for module tools");
    // baml_src/ should not be created (build pipeline only writes when functions.is_empty() == false)
    assert!(!dir.path().join("generated/baml_src").exists());
}

#[test]
fn test_codegen_baml_files_generated() {
    let input = r#"
type KeywordList {
    keywords: list<string>
}

client LocalQwen {
    provider = "local"
    model = "local.qwen2.5-coder:7b"
}

tool ExtractKeywords(text: string) -> KeywordList {
    invoke: baml("ExtractKeywords")
}
"#;
    let doc = parser::parse(input).expect("parse failed");
    let dir = tempdir().unwrap();

    // Generate BAML output
    let baml_output = codegen::generate_baml(&doc).expect("baml codegen failed");
    assert!(!baml_output.functions.is_empty(), "should generate BAML function");

    // Write like the build pipeline does
    let baml_dir = dir.path().join("generated/baml_src");
    fs::create_dir_all(&baml_dir).unwrap();
    fs::write(baml_dir.join("functions.baml"), &baml_output.functions).unwrap();
    fs::write(baml_dir.join("types.baml"), &baml_output.types).unwrap();
    fs::write(baml_dir.join("clients.baml"), &baml_output.clients).unwrap();

    // Verify functions.baml
    let functions = fs::read_to_string(baml_dir.join("functions.baml")).unwrap();
    assert!(functions.contains("function ExtractKeywords"), "should define BAML function");
    // Must use actual client name, NOT hardcoded "DefaultClient"
    assert!(functions.contains("client LocalQwen"), "should reference actual client name");
    assert!(!functions.contains("DefaultClient"), "must NOT use hardcoded DefaultClient");

    // Verify types.baml
    let types = fs::read_to_string(baml_dir.join("types.baml")).unwrap();
    assert!(types.contains("class KeywordList"), "should define KeywordList class");

    // Verify clients.baml
    let clients = fs::read_to_string(baml_dir.join("clients.baml")).unwrap();
    assert!(clients.contains("client<llm> LocalQwen"), "should define LocalQwen client");

    // Verify MCP server emits baml_client handler (not stub)
    codegen::generate_mcp(&doc, dir.path()).expect("mcp codegen failed");
    let mcp = fs::read_to_string(dir.path().join("generated/mcp-server.js")).unwrap();
    assert!(mcp.contains("baml_client/index.js"), "BAML tool handler must import baml_client");
    assert!(!mcp.contains("scripts/stub"), "BAML tool must NOT fall back to scripts/stub");
}

// ── Spec 40: runtime.js codegen tests ────────────────────────────────────────

#[test]
fn test_runtime_js_generated_on_build() {
    let doc = parser::parse(FULL_DOC).expect("parse failed");
    semantic::analyze(&doc).expect("semantic failed");

    let dir = tempdir().unwrap();
    codegen::generate_runtime(&doc, dir.path()).expect("generate_runtime failed");

    assert!(
        dir.path().join("generated/runtime.js").exists(),
        "generate_runtime must write generated/runtime.js"
    );
}

#[test]
fn test_runtime_js_zero_npm_imports() {
    let doc = parser::parse(FULL_DOC).expect("parse failed");
    semantic::analyze(&doc).expect("semantic failed");

    let dir = tempdir().unwrap();
    codegen::generate_runtime(&doc, dir.path()).expect("generate_runtime failed");

    let content = fs::read_to_string(dir.path().join("generated/runtime.js")).unwrap();

    assert!(!content.contains("require("), "runtime.js must not use require()");
    assert!(!content.contains("@anthropic-ai/sdk"), "runtime.js must not import @anthropic-ai/sdk");
    assert!(!content.contains("@modelcontextprotocol/sdk"), "runtime.js must not import @modelcontextprotocol/sdk");
    assert!(content.contains("fetch("), "runtime.js must use raw fetch()");
}

#[test]
fn test_runtime_js_list_flag_emitted() {
    let doc = parser::parse(FULL_DOC).expect("parse failed");
    semantic::analyze(&doc).expect("semantic failed");

    let dir = tempdir().unwrap();
    codegen::generate_runtime(&doc, dir.path()).expect("generate_runtime failed");

    let content = fs::read_to_string(dir.path().join("generated/runtime.js")).unwrap();

    assert!(content.contains("--list"), "runtime.js must handle --list flag");
    assert!(content.contains("Object.values(PLANS)"), "runtime.js --list must enumerate PLANS");
}

#[test]
fn test_runtime_js_task_template_is_plain_string() {
    let doc = parser::parse(FULL_DOC).expect("parse failed");
    semantic::analyze(&doc).expect("semantic failed");

    let dir = tempdir().unwrap();
    codegen::generate_runtime(&doc, dir.path()).expect("generate_runtime failed");

    let content = fs::read_to_string(dir.path().join("generated/runtime.js")).unwrap();

    // taskTemplate must be a plain double-quoted string so interpolate() can expand ${var} at runtime.
    // Template literals (backticks) would evaluate ${topic} at JS parse time against module scope.
    assert!(
        content.contains(r#"taskTemplate: "find "#),
        "taskTemplate must be a double-quoted string, not a template literal"
    );
    assert!(
        !content.contains("taskTemplate: `"),
        "taskTemplate must NOT be a backtick template literal"
    );
}

#[test]
fn test_runtime_js_node_syntax_check() {
    let doc = parser::parse(FULL_DOC).expect("parse failed");
    semantic::analyze(&doc).expect("semantic failed");

    let dir = tempdir().unwrap();
    codegen::generate_runtime(&doc, dir.path()).expect("generate_runtime failed");

    let node = match find_node() {
        Some(n) => n,
        None => { println!("skip: node not found"); return; }
    };

    let status = Command::new(&node)
        .arg("--check")
        .arg(dir.path().join("generated/runtime.js"))
        .status()
        .unwrap();

    assert!(status.success(), "runtime.js must pass node --check (no syntax errors)");
}

#[test]
fn test_runtime_js_list_flag_valid_json() {
    let doc = parser::parse(FULL_DOC).expect("parse failed");
    semantic::analyze(&doc).expect("semantic failed");

    let dir = tempdir().unwrap();
    codegen::generate_runtime(&doc, dir.path()).expect("generate_runtime failed");

    let node = match find_node() {
        Some(n) => n,
        None => { println!("skip: node not found"); return; }
    };

    let output = Command::new(&node)
        .arg(dir.path().join("generated/runtime.js"))
        .arg("--list")
        .output()
        .unwrap();

    assert!(output.status.success(), "--list must exit 0");

    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
        .expect("--list output must be valid JSON");
    let arr = parsed.as_array().expect("--list output must be a JSON array");
    assert!(!arr.is_empty(), "--list must contain at least one workflow");
    assert!(arr[0]["name"].is_string(), "each workflow must have a 'name' field");
    assert!(arr[0]["requiredArgs"].is_array(), "each workflow must have a 'requiredArgs' field");
}

#[test]
fn test_runtime_js_no_workflows_emits_empty_plans() {
    let input = r#"
client C {
    provider = "anthropic"
    model = "claude-4-sonnet"
}
agent A {
    client = C
}
"#;
    let doc = parser::parse(input).expect("parse failed");
    let dir = tempdir().unwrap();
    codegen::generate_runtime(&doc, dir.path()).expect("generate_runtime failed");

    let content = fs::read_to_string(dir.path().join("generated/runtime.js")).unwrap();
    assert!(
        content.contains("const PLANS = {};"),
        "no workflows must produce empty PLANS object (got: {})",
        &content[content.find("const PLANS").unwrap_or(0)..
                  (content.find("const PLANS").unwrap_or(0) + 80).min(content.len())]
    );
}
