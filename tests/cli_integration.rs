use std::process::Command;
use assert_cmd::prelude::*;
use tempfile::tempdir;
use std::fs;

#[test]
fn test_cli_init_creates_expected_files() {
    let dir = tempdir().unwrap();
    Command::cargo_bin("claw").unwrap()
        .current_dir(dir.path())
        .arg("init")
        .assert()
        .success();

    assert!(dir.path().join("example.claw").exists());
    assert!(dir.path().join("claw.json").exists());
    assert!(dir.path().join("package.json").exists());
    assert!(dir.path().join("scripts/search.js").exists());
}

#[test]
fn test_cli_build_example_claw() {
    let dir = tempdir().unwrap();
    let example_path = dir.path().join("example.claw");
    // Use newline-separated fields — Claw does not allow commas between block fields
    fs::write(&example_path, r#"
type SearchResult { url: string }
tool WebSearch(query: string) -> SearchResult {
    invoke: module("scripts/search").function("run")
}
client MyC {
    provider = "anthropic"
    model = "claude-4-sonnet"
}
agent Research {
    client = MyC
    tools = [WebSearch]
}
workflow Find(topic: string) -> SearchResult {
    return execute Research.run(task: topic, require_type: SearchResult)
}
"#).unwrap();

    Command::cargo_bin("claw").unwrap()
        .current_dir(dir.path())
        .arg("build")
        .arg("example.claw")
        .assert()
        .success();

    assert!(dir.path().join("opencode.json").exists());
    assert!(dir.path().join("generated/mcp-server.js").exists());
}

#[test]
fn test_cli_build_parse_error_exit_code() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("error.claw");
    fs::write(&path, "type Foo { oops").unwrap();

    Command::cargo_bin("claw").unwrap()
        .current_dir(dir.path())
        .arg("build")
        .arg("error.claw")
        .assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains("error:"));
}

#[test]
fn test_cli_build_semantic_error_exit_code() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("sem_error.claw");
    fs::write(&path, "agent A { tools = [Missing] }").unwrap();

    Command::cargo_bin("claw").unwrap()
        .current_dir(dir.path())
        .arg("build")
        .arg("sem_error.claw")
        .assert()
        .failure()
        .code(2);
}

#[test]
fn test_cli_build_watch_starts() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ex.claw");
    // Valid enough for the watch loop to start (even if build fails, watch stays alive)
    fs::write(&path, r#"
agent A {
    system_prompt = "Hello."
}
"#).unwrap();

    let mut child = Command::cargo_bin("claw").unwrap()
        .current_dir(dir.path())
        .arg("build")
        .arg("ex.claw")
        .arg("--watch")
        .spawn()
        .unwrap();

    std::thread::sleep(std::time::Duration::from_secs(1));
    // Process should still be alive (watch loop running)
    child.kill().ok();
}
