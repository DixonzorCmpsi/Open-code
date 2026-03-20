use std::process::Command;
use assert_cmd::prelude::*;
use tempfile::tempdir;
use std::fs;

mod helpers;
use helpers::find_node;

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
    assert!(
        dir.path().join("generated/runtime.js").exists(),
        "claw build must generate runtime.js"
    );
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

// ── Spec 40: claw run + claw init regression tests ───────────────────────────

#[test]
fn test_cli_run_missing_runtime() {
    // In an empty directory with no generated/runtime.js, claw run must fail
    // with an actionable message telling the user to run claw build.
    let dir = tempdir().unwrap();

    Command::cargo_bin("claw").unwrap()
        .current_dir(dir.path())
        .arg("run")
        .arg("FindInfo")
        .assert()
        .failure()
        .stderr(predicates::str::contains("claw build"));
}

#[test]
fn test_cli_run_list_flag() {
    // Build a minimal .claw file, then claw run --list must output a JSON
    // array containing the workflow name. Skipped if Node.js is not found.
    let dir = tempdir().unwrap();
    let path = dir.path().join("example.claw");
    fs::write(&path, r#"
client C { provider = "anthropic" model = "claude-4-sonnet" }
agent A { client = C }
workflow FindInfo(topic: string) {
    return execute A.run(task: topic)
}
"#).unwrap();

    Command::cargo_bin("claw").unwrap()
        .current_dir(dir.path())
        .arg("build")
        .arg("example.claw")
        .assert()
        .success();

    let node = match find_node() {
        Some(n) => n,
        None => { println!("skip: node not found"); return; }
    };

    // Pass --list as the workflow name — runtime.js interprets it as --list flag
    let output = Command::new(&node)
        .arg(dir.path().join("generated/runtime.js"))
        .arg("--list")
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success(), "--list must exit 0");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("FindInfo"), "--list output must contain workflow name FindInfo");
}

#[test]
fn test_cli_init_no_opencode_message() {
    // claw init must promote claw run as the primary execution path.
    // It must NOT reference opencode run --command (Spec 39 architecture inversion).
    let dir = tempdir().unwrap();
    let out = Command::cargo_bin("claw").unwrap()
        .current_dir(dir.path())
        .arg("init")
        .output()
        .unwrap();

    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("claw run"),
        "init must mention 'claw run' as primary path"
    );
    assert!(
        !stdout.contains("opencode run --command"),
        "init must NOT mention 'opencode run --command'"
    );
}

#[test]
fn test_cli_init_package_json_optional_deps() {
    // The generated package.json must place SDK packages in optionalDependencies,
    // not dependencies — npm install is optional for claw run (Spec 39 §5).
    let dir = tempdir().unwrap();
    Command::cargo_bin("claw").unwrap()
        .current_dir(dir.path())
        .arg("init")
        .assert()
        .success();

    let content = fs::read_to_string(dir.path().join("package.json")).unwrap();
    let pkg: serde_json::Value = serde_json::from_str(&content).unwrap();

    assert!(
        pkg["optionalDependencies"]["@anthropic-ai/sdk"].is_string(),
        "@anthropic-ai/sdk must be in optionalDependencies"
    );
    assert!(
        pkg["optionalDependencies"]["@modelcontextprotocol/sdk"].is_string(),
        "@modelcontextprotocol/sdk must be in optionalDependencies"
    );
    // dependencies block must not exist or must be empty
    let deps_empty = pkg["dependencies"].is_null()
        || pkg["dependencies"].as_object().map(|o| o.is_empty()).unwrap_or(true);
    assert!(
        deps_empty,
        "dependencies block must not contain SDK packages"
    );
}
