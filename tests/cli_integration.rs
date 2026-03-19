use std::process::Command;
use assert_cmd::prelude::*;
use tempfile::tempdir;
use std::fs;

#[test]
fn test_cli_init_creates_expected_files() {
    let dir = tempdir().unwrap();
    let mut cmd = Command::cargo_bin("claw").unwrap();
    cmd.current_dir(dir.path())
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
    fs::write(&example_path, r#"
type SearchResult { url: string }
tool WebSearch(query: string) -> SearchResult { invoke: module("a").function("b") }
client MyC { provider = "anthropic", model = "claude-4-sonnet" }
agent Research { client = MyC, tools = [WebSearch] }
workflow Find(topic: string) -> SearchResult {
    return execute Research.run(task: topic, require_type: SearchResult)
}
"#).unwrap();

    let mut cmd = Command::cargo_bin("claw").unwrap();
    cmd.current_dir(dir.path())
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

    let mut cmd = Command::cargo_bin("claw").unwrap();
    cmd.current_dir(dir.path())
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

    let mut cmd = Command::cargo_bin("claw").unwrap();
    cmd.current_dir(dir.path())
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
    fs::write(&path, "agent A {}").unwrap();

    let mut cmd = Command::cargo_bin("claw").unwrap();
    let child = cmd.current_dir(dir.path())
       .arg("build")
       .arg("ex.claw")
       .arg("--watch")
       .spawn()
       .unwrap();

    std::thread::sleep(std::time::Duration::from_secs(1));
    // Kill the process. If it didn't crash, we're good.
    let mut child = child;
    child.kill().ok();
}
