use std::process::Command;

fn berth() -> Command {
    Command::new(env!("CARGO_BIN_EXE_berth"))
}

fn berth_with_home(tmp: &std::path::Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_berth"));
    cmd.env("BERTH_HOME", tmp.join(".berth"));
    cmd
}

fn patch_runtime_to_long_running(tmp: &std::path::Path, server: &str) {
    let config_path = tmp.join(".berth/servers").join(format!("{server}.toml"));
    let content = std::fs::read_to_string(&config_path).unwrap();
    let mut value: toml::Value = toml::from_str(&content).unwrap();
    let runtime = value
        .get_mut("runtime")
        .and_then(toml::Value::as_table_mut)
        .unwrap();

    #[cfg(unix)]
    {
        runtime.insert("command".to_string(), toml::Value::String("sh".to_string()));
        runtime.insert(
            "args".to_string(),
            toml::Value::Array(vec![
                toml::Value::String("-c".to_string()),
                toml::Value::String("sleep 60".to_string()),
            ]),
        );
    }

    #[cfg(windows)]
    {
        runtime.insert(
            "command".to_string(),
            toml::Value::String("cmd".to_string()),
        );
        runtime.insert(
            "args".to_string(),
            toml::Value::Array(vec![
                toml::Value::String("/C".to_string()),
                toml::Value::String("timeout /T 60 /NOBREAK".to_string()),
            ]),
        );
    }

    let rendered = toml::to_string_pretty(&value).unwrap();
    std::fs::write(&config_path, rendered).unwrap();
}

fn patch_installed_version(tmp: &std::path::Path, server: &str, version: &str) {
    let config_path = tmp.join(".berth/servers").join(format!("{server}.toml"));
    let content = std::fs::read_to_string(&config_path).unwrap();
    let mut value: toml::Value = toml::from_str(&content).unwrap();
    value
        .get_mut("server")
        .and_then(toml::Value::as_table_mut)
        .unwrap()
        .insert(
            "version".to_string(),
            toml::Value::String(version.to_string()),
        );
    let rendered = toml::to_string_pretty(&value).unwrap();
    std::fs::write(&config_path, rendered).unwrap();
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
    let tmp = tempfile::tempdir().unwrap();
    let output = berth_with_home(tmp.path()).args(["list"]).output().unwrap();
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

// --- install ---

#[test]
fn install_creates_config_file() {
    let tmp = tempfile::tempdir().unwrap();
    let output = berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Installed"));

    let config_path = tmp.path().join(".berth/servers/github.toml");
    assert!(config_path.exists());
}

#[test]
fn install_already_installed_warns() {
    let tmp = tempfile::tempdir().unwrap();
    // Install once
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();

    // Install again
    let output = berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("already installed"));
}

#[test]
fn install_not_found_exits_1() {
    let tmp = tempfile::tempdir().unwrap();
    let output = berth_with_home(tmp.path())
        .args(["install", "nonexistent"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found"));
}

#[test]
fn install_suggests_config() {
    let tmp = tempfile::tempdir().unwrap();
    let output = berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("berth config"));
    assert!(stdout.contains("token"));
}

// --- uninstall ---

#[test]
fn uninstall_removes_config_file() {
    let tmp = tempfile::tempdir().unwrap();
    // Install first
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();

    let config_path = tmp.path().join(".berth/servers/github.toml");
    assert!(config_path.exists());

    // Uninstall
    let output = berth_with_home(tmp.path())
        .args(["uninstall", "github"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!config_path.exists());
}

#[test]
fn uninstall_not_installed_exits_1() {
    let tmp = tempfile::tempdir().unwrap();
    let output = berth_with_home(tmp.path())
        .args(["uninstall", "github"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not installed"));
}

// --- config ---

#[test]
fn config_not_installed_exits_1() {
    let tmp = tempfile::tempdir().unwrap();
    let output = berth_with_home(tmp.path())
        .args(["config", "github"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not installed"));
}

#[test]
fn config_show_lists_required() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();

    let output = berth_with_home(tmp.path())
        .args(["config", "github"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("token"));
    assert!(stdout.contains("NOT SET"));
}

#[test]
fn config_set_updates_value() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();

    let output = berth_with_home(tmp.path())
        .args(["config", "github", "--set", "token=abc123"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let config_path = tmp.path().join(".berth/servers/github.toml");
    let content = std::fs::read_to_string(config_path).unwrap();
    assert!(content.contains("abc123"));
}

#[test]
fn config_set_unknown_key_exits_1() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();

    let output = berth_with_home(tmp.path())
        .args(["config", "github", "--set", "unknown_key=value"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn config_env_shows_variables() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();

    let output = berth_with_home(tmp.path())
        .args(["config", "github", "--env"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("GITHUB_TOKEN"));
}

// --- list with version ---

#[test]
fn list_shows_version_after_install() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();

    let output = berth_with_home(tmp.path()).args(["list"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("VERSION"));
    assert!(stdout.contains("1.2.0"));
}

// --- runtime lifecycle ---

#[test]
fn start_not_installed_exits_1() {
    let tmp = tempfile::tempdir().unwrap();
    let output = berth_with_home(tmp.path())
        .args(["start", "github"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not installed"));
}

#[test]
fn start_requires_config_before_running() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();

    let output = berth_with_home(tmp.path())
        .args(["start", "github"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Missing required config"));
}

#[test]
fn start_then_status_shows_running() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();
    berth_with_home(tmp.path())
        .args(["config", "github", "--set", "token=abc123"])
        .output()
        .unwrap();
    patch_runtime_to_long_running(tmp.path(), "github");

    let start = berth_with_home(tmp.path())
        .args(["start", "github"])
        .output()
        .unwrap();
    assert!(start.status.success());

    let status = berth_with_home(tmp.path())
        .args(["status"])
        .output()
        .unwrap();
    assert!(status.status.success());
    let stdout = String::from_utf8_lossy(&status.stdout);
    assert!(stdout.contains("github"));
    assert!(stdout.contains("running"));
}

#[test]
fn stop_after_start_shows_stopped() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();
    berth_with_home(tmp.path())
        .args(["config", "github", "--set", "token=abc123"])
        .output()
        .unwrap();
    patch_runtime_to_long_running(tmp.path(), "github");
    berth_with_home(tmp.path())
        .args(["start", "github"])
        .output()
        .unwrap();

    let stop = berth_with_home(tmp.path())
        .args(["stop", "github"])
        .output()
        .unwrap();
    assert!(stop.status.success());

    let status = berth_with_home(tmp.path())
        .args(["status"])
        .output()
        .unwrap();
    assert!(status.status.success());
    let stdout = String::from_utf8_lossy(&status.stdout);
    assert!(stdout.contains("stopped"));
}

#[test]
fn restart_sets_running_state() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();
    berth_with_home(tmp.path())
        .args(["config", "github", "--set", "token=abc123"])
        .output()
        .unwrap();
    patch_runtime_to_long_running(tmp.path(), "github");

    let restart = berth_with_home(tmp.path())
        .args(["restart", "github"])
        .output()
        .unwrap();
    assert!(restart.status.success());

    let status = berth_with_home(tmp.path())
        .args(["status"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&status.stdout);
    assert!(stdout.contains("running"));
}

#[test]
fn logs_show_lifecycle_events() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();
    berth_with_home(tmp.path())
        .args(["config", "github", "--set", "token=abc123"])
        .output()
        .unwrap();
    patch_runtime_to_long_running(tmp.path(), "github");
    berth_with_home(tmp.path())
        .args(["start", "github"])
        .output()
        .unwrap();
    berth_with_home(tmp.path())
        .args(["stop", "github"])
        .output()
        .unwrap();

    let logs = berth_with_home(tmp.path())
        .args(["logs", "github", "--tail", "10"])
        .output()
        .unwrap();
    assert!(logs.status.success());
    let stdout = String::from_utf8_lossy(&logs.stdout);
    assert!(stdout.contains("START"));
    assert!(stdout.contains("STOP"));
}

// --- client linking ---

#[test]
fn link_claude_desktop_writes_config() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();
    berth_with_home(tmp.path())
        .args(["config", "github", "--set", "token=abc123"])
        .output()
        .unwrap();

    let output = berth_with_home(tmp.path())
        .args(["link", "claude-desktop"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let config_path = tmp
        .path()
        .join(".berth/clients/claude-desktop/claude_desktop_config.json");
    assert!(config_path.exists());

    let content = std::fs::read_to_string(config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    let github = &json["mcpServers"]["github"];
    assert_eq!(github["command"], "npx");
    assert_eq!(github["env"]["GITHUB_TOKEN"], "abc123");
}

#[test]
fn link_claude_desktop_creates_backup_when_file_exists() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();
    berth_with_home(tmp.path())
        .args(["config", "github", "--set", "token=abc123"])
        .output()
        .unwrap();

    // First write creates config
    berth_with_home(tmp.path())
        .args(["link", "claude-desktop"])
        .output()
        .unwrap();
    // Second write should create backup
    let output = berth_with_home(tmp.path())
        .args(["link", "claude-desktop"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let backup = tmp
        .path()
        .join(".berth/clients/claude-desktop/claude_desktop_config.json.bak");
    assert!(backup.exists());
}

#[test]
fn unlink_claude_desktop_removes_linked_servers() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();
    berth_with_home(tmp.path())
        .args(["config", "github", "--set", "token=abc123"])
        .output()
        .unwrap();
    berth_with_home(tmp.path())
        .args(["link", "claude-desktop"])
        .output()
        .unwrap();

    let output = berth_with_home(tmp.path())
        .args(["unlink", "claude-desktop"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let config_path = tmp
        .path()
        .join(".berth/clients/claude-desktop/claude_desktop_config.json");
    let content = std::fs::read_to_string(config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(json["mcpServers"]["github"].is_null());
}

#[test]
fn link_unknown_client_exits_1() {
    let output = berth().args(["link", "cursor"]).output().unwrap();
    assert!(!output.status.success());
}

// --- update ---

#[test]
fn update_without_args_exits_1() {
    let output = berth().args(["update"]).output().unwrap();
    assert!(!output.status.success());
}

#[test]
fn update_specific_server_updates_and_preserves_config() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();
    berth_with_home(tmp.path())
        .args(["config", "github", "--set", "token=abc123"])
        .output()
        .unwrap();
    patch_installed_version(tmp.path(), "github", "0.9.0");

    let output = berth_with_home(tmp.path())
        .args(["update", "github"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Updated"));
    assert!(stdout.contains("0.9.0"));
    assert!(stdout.contains("1.2.0"));

    let updated = std::fs::read_to_string(tmp.path().join(".berth/servers/github.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&updated).unwrap();
    assert_eq!(parsed["server"]["version"].as_str(), Some("1.2.0"));
    assert_eq!(parsed["config"]["token"].as_str(), Some("abc123"));
}

#[test]
fn update_specific_server_up_to_date_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();

    let output = berth_with_home(tmp.path())
        .args(["update", "github"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("already up to date"));
}

#[test]
fn update_non_installed_server_exits_1() {
    let tmp = tempfile::tempdir().unwrap();
    let output = berth_with_home(tmp.path())
        .args(["update", "github"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not installed"));
}

#[test]
fn update_all_mixed_results_exits_1_and_reports_summary() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();
    patch_installed_version(tmp.path(), "github", "0.9.0");

    // Add an installed server entry that does not exist in the registry.
    let servers_dir = tmp.path().join(".berth/servers");
    let github_cfg = std::fs::read_to_string(servers_dir.join("github.toml")).unwrap();
    std::fs::write(servers_dir.join("missing.toml"), github_cfg).unwrap();

    let output = berth_with_home(tmp.path())
        .args(["update", "--all"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("Updated"));
    assert!(stdout.contains("failed"));
    assert!(stderr.contains("not found in the registry"));
}
