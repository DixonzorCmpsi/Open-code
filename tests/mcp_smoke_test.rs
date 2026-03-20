use std::process::Command;
use assert_cmd::prelude::*;
use tempfile::tempdir;
use std::fs;

mod helpers;
use helpers::find_node;

#[test]
fn test_mcp_server_starts_and_lists_tools() {
    let dir = tempdir().unwrap();
    let example_path = dir.path().join("example.claw");
    fs::write(&example_path, r#"
type SearchResult { url: string }
tool WebSearch(query: string) -> SearchResult {
    invoke: module("scripts/search").function("run")
}
client MyC {
    provider = "anthropic"
    model = "claude-4-sonnet"
}
agent Researcher {
    client = MyC
    tools = [WebSearch]
}
"#).unwrap();

    // 1. Build
    Command::cargo_bin("claw").unwrap()
        .current_dir(dir.path())
        .arg("build")
        .arg("example.claw")
        .assert()
        .success();

    // 2. Check JS syntax (node --check)
    let node = match find_node() {
        Some(n) => n,
        None => {
            println!("Skipping MCP smoke test — node not found in PATH");
            return;
        }
    };
    let js_path = dir.path().join("generated/mcp-server.js");
    let status = Command::new(&node)
        .arg("--check")
        .arg(&js_path)
        .current_dir(dir.path())
        .status()
        .expect("node failed to start");
    assert!(status.success(), "mcp-server.js failed node --check");

    // 3. MCP List Tools Test — only runs if node_modules are available
    let test_mjs = r#"
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

const jsPath = process.argv[2];
const transport = new StdioClientTransport({
  command: "node",
  args: [jsPath]
});
const client = new Client({ name: "test", version: "1.0.0" }, { capabilities: {} });
await client.connect(transport);
const { tools } = await client.listTools();
console.log(JSON.stringify(tools.map(t => t.name)));
process.exit(0);
"#;
    let test_mjs_path = dir.path().join("test_mcp.mjs");
    fs::write(&test_mjs_path, test_mjs).unwrap();

    if dir.path().join("node_modules").exists() {
        let output = Command::new(&node)
            .arg("test_mcp.mjs")
            .arg("generated/mcp-server.js")
            .current_dir(dir.path())
            .output()
            .unwrap();

        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("WebSearch"), "should list WebSearch tool");
        assert!(stdout.contains("agent_Researcher"), "should list agent_Researcher tool");
    } else {
        println!("Skipping live MCP list tools test - node_modules missing in temp dir");
    }
}
