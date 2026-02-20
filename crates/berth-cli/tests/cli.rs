use std::process::Command;

fn berth() -> Command {
    Command::new(env!("CARGO_BIN_EXE_berth"))
}

// --- search ---

#[test]
fn search_finds_server() {
    let output = berth().args(["search", "github"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("github"));
}

#[test]
fn search_no_results() {
    let output = berth().args(["search", "nonexistent"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No servers found"));
}

#[test]
fn search_case_insensitive() {
    let output = berth().args(["search", "GitHub"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("github"));
}

#[test]
fn search_multiple_results() {
    // "server" appears in every display name ("... MCP Server")
    let output = berth().args(["search", "server"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("github"));
    assert!(stdout.contains("filesystem"));
}

// --- info ---

#[test]
fn info_shows_details() {
    let output = berth().args(["info", "github"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("GitHub MCP Server"));
    assert!(stdout.contains("Anthropic"));
    assert!(stdout.contains("official"));
}

#[test]
fn info_not_found() {
    let output = berth().args(["info", "nonexistent"]).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found"));
}

// --- list ---

#[test]
fn list_no_servers() {
    let output = berth().args(["list"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No servers installed"));
}

// --- help & version ---

#[test]
fn help_shows_subcommands() {
    let output = berth().arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("search"));
    assert!(stdout.contains("install"));
    assert!(stdout.contains("start"));
}

#[test]
fn version_flag() {
    let output = berth().arg("--version").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("0.1.0"));
}

// --- stubs ---

#[test]
fn stub_install_prints_not_implemented() {
    let output = berth().args(["install", "foo"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("not yet implemented"));
    assert!(stdout.contains("foo"));
}

#[test]
fn stub_uninstall_prints_not_implemented() {
    let output = berth().args(["uninstall", "foo"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("not yet implemented"));
}

#[test]
fn stub_start_prints_not_implemented() {
    let output = berth().args(["start", "foo"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("not yet implemented"));
}

#[test]
fn stub_link_prints_not_implemented() {
    let output = berth().args(["link", "claude-desktop"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("not yet implemented"));
}
