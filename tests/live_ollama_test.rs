use std::process::Command;
use assert_cmd::prelude::*;
use tempfile::tempdir;
use std::fs;
use reqwest::{blocking::get, StatusCode};

#[test]
#[ignore]
fn test_ollama_is_running() {
    let res = get("http://localhost:11434/v1/models").expect("Failed to connect to local Ollama");
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = res.json().expect("Failed to parse JSON response");
    let models = body["data"].as_array().expect("Models not an array");
    let has_qwen = models.iter().any(|m| m["id"].as_str().map_or(false, |s| s.contains("qwen2.5-coder:7b")));
    assert!(has_qwen, "Ollama is missing 'qwen2.5-coder:7b' model");
}

#[test]
#[ignore]
fn test_live_e2e_with_local_qwen() {
    let dir = tempdir().unwrap();
    let example_path = dir.path().join("example.claw");
    fs::write(&example_path, r#"
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

agent Reporter {
    client = LocalQwen
    system_prompt = "You report the confidence scores for search results."
    tools = [WebSearch]
}

workflow Find(topic: string) -> SearchResult {
    return execute Reporter.run(task: topic, require_type: SearchResult)
}
"#).unwrap();

    // 1. Build
    Command::cargo_bin("claw").unwrap()
        .current_dir(dir.path())
        .arg("build")
        .arg("example.claw")
        .assert()
        .success();

    // 2. Prep node environment
    fs::write(dir.path().join("package.json"), r#"{
  "name": "test-env",
  "type": "module",
  "dependencies": {
    "@modelcontextprotocol/sdk": "^1.12.0"
  }
}"#).unwrap();

    Command::new("npm")
        .arg("install")
        .current_dir(dir.path())
        .output()
        .expect("npm install failed");

    // 3. Run OpenCode non-interactively
    // Note: slash commands (/Find) are TUI-only — use -p flag for CLI
    let opencode_run = Command::new("opencode")
        .arg("-p")
        .arg("Find info about quantum computing")
        .arg("-q")
        .env("LOCAL_ENDPOINT", "http://localhost:11434")
        .current_dir(dir.path())
        .output()
        .expect("opencode failed to start");

    let stdout = String::from_utf8(opencode_run.stdout).unwrap();
    assert!(opencode_run.status.success(), "opencode failed: {}", stdout);
}
