use std::process::Command;
use assert_cmd::prelude::*;
use tempfile::tempdir;
use std::fs;
use reqwest::{blocking::get, StatusCode};

mod helpers;
use helpers::find_node;

#[test]
#[ignore]
fn test_ollama_is_running() {
    let res = get("http://localhost:11434/v1/models").expect("Failed to connect to local Ollama");
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = res.json().expect("Failed to parse JSON response");
    let models = body["data"].as_array().expect("Models not an array");
    // qwen2.5:14b is the verified model for tool calling (Spec 39 real test session)
    let has_qwen = models.iter().any(|m| m["id"].as_str().map_or(false, |s| s.contains("qwen2.5:14b")));
    assert!(has_qwen, "Ollama is missing 'qwen2.5:14b' model");
}

#[test]
#[ignore]
fn test_live_e2e_claw_run() {
    // End-to-end test using claw run (not opencode) — Spec 39 architecture.
    // Run manually: cargo test -- --ignored test_live_e2e_claw_run
    // Requires: Ollama at localhost:11434 with qwen2.5:14b loaded.
    let node = find_node().expect("Node.js not found — install Node.js >= 18");

    let dir = tempdir().unwrap();
    let example_path = dir.path().join("example.claw");

    // No-tools summarizer agent — verified to work against qwen2.5:14b in Spec 39 session.
    fs::write(&example_path, r#"
client LocalQwen {
    provider = "local"
    model = "local.qwen2.5:14b"
}

agent Summarizer {
    client = LocalQwen
    system_prompt = "You are a concise summarizer. Respond in 2-3 sentences."
    settings = { max_steps: 2, temperature: 0.3 }
}

workflow Summarize(topic: string) {
    let result = execute Summarizer.run(
        task: "Summarize ${topic} in 2-3 sentences."
    )
    return result
}
"#).unwrap();

    // 1. Build — must generate runtime.js with zero npm deps
    Command::cargo_bin("claw").unwrap()
        .current_dir(dir.path())
        .arg("build")
        .arg("example.claw")
        .assert()
        .success();

    assert!(
        dir.path().join("generated/runtime.js").exists(),
        "claw build must produce generated/runtime.js"
    );

    // 2. Verify --list output is valid JSON containing the workflow
    let list_output = Command::new(&node)
        .arg(dir.path().join("generated/runtime.js"))
        .arg("--list")
        .current_dir(dir.path())
        .output()
        .expect("node --list failed");

    assert!(list_output.status.success(), "--list must exit 0");
    let list_str = String::from_utf8(list_output.stdout).unwrap();
    assert!(list_str.contains("Summarize"), "--list must include Summarize workflow");

    // 3. Run workflow via claw run — no npm install, no opencode
    let run_output = Command::cargo_bin("claw").unwrap()
        .current_dir(dir.path())
        .arg("run")
        .arg("Summarize")
        .arg("--arg")
        .arg("topic=quantum computing")
        .output()
        .expect("claw run failed");

    let stdout = String::from_utf8(run_output.stdout).unwrap();
    assert!(
        run_output.status.success(),
        "claw run must exit 0; stdout: {stdout}"
    );
    assert!(!stdout.trim().is_empty(), "claw run must produce output");
}
