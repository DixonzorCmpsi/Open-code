use std::process::Command;

/// Find a usable Node.js binary. Returns the path as a String, or None if not found.
/// Checks common install locations before falling back to PATH lookup.
pub fn find_node() -> Option<String> {
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
