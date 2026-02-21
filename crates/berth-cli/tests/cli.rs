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

fn patch_runtime_to_echo(tmp: &std::path::Path, server: &str) {
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
                toml::Value::String("echo proxy-ok".to_string()),
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
                toml::Value::String("echo proxy-ok".to_string()),
            ]),
        );
    }

    let rendered = toml::to_string_pretty(&value).unwrap();
    std::fs::write(&config_path, rendered).unwrap();
}

fn patch_runtime_to_print_env_var(tmp: &std::path::Path, server: &str, env_var: &str) {
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
                toml::Value::String(format!(
                    "if [ -n \"${env_var}\" ]; then echo env-present; else echo env-missing; fi"
                )),
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
                toml::Value::String(format!(
                    "if defined {env_var} (echo env-present) else (echo env-missing)"
                )),
            ]),
        );
    }

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

#[test]
fn install_specific_version_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let output = berth_with_home(tmp.path())
        .args(["install", "github@1.2.0"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Installed"));
    assert!(stdout.contains("v1.2.0"));
}

#[test]
fn install_unavailable_version_exits_1() {
    let tmp = tempfile::tempdir().unwrap();
    let output = berth_with_home(tmp.path())
        .args(["install", "github@9.9.9"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not available"));
}

#[test]
fn install_invalid_server_spec_exits_1() {
    let tmp = tempfile::tempdir().unwrap();
    let output = berth_with_home(tmp.path())
        .args(["install", "github@"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Invalid server format"));
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

#[test]
fn config_export_import_round_trip() {
    let source = tempfile::tempdir().unwrap();
    berth_with_home(source.path())
        .args(["install", "github"])
        .output()
        .unwrap();
    berth_with_home(source.path())
        .args(["config", "github", "--set", "token=abc123"])
        .output()
        .unwrap();

    let export_file = source.path().join("team-berth.toml");
    let export = berth_with_home(source.path())
        .args(["config", "export", export_file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(export.status.success());
    assert!(export_file.exists());

    let target = tempfile::tempdir().unwrap();
    berth_with_home(target.path())
        .args(["install", "github"])
        .output()
        .unwrap();

    let import = berth_with_home(target.path())
        .args(["config", "import", export_file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(import.status.success());

    let target_config =
        std::fs::read_to_string(target.path().join(".berth/servers/github.toml")).unwrap();
    assert!(target_config.contains("abc123"));
}

#[test]
fn config_import_requires_file() {
    let tmp = tempfile::tempdir().unwrap();
    let output = berth_with_home(tmp.path())
        .args(["config", "import"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Missing import file"));
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
fn status_shows_pid_for_running_server() {
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

    let state_path = tmp.path().join(".berth/runtime/github.toml");
    let state_content = std::fs::read_to_string(state_path).unwrap();
    let state: toml::Value = toml::from_str(&state_content).unwrap();
    let pid = state["pid"].as_integer().unwrap().to_string();

    let status = berth_with_home(tmp.path())
        .args(["status"])
        .output()
        .unwrap();
    assert!(status.status.success());
    let stdout = String::from_utf8_lossy(&status.stdout);
    assert!(stdout.contains("PID"));
    assert!(stdout.contains("MEMORY"));
    assert!(stdout.contains(&pid));
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
fn status_malformed_runtime_state_exits_1() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();
    let runtime_dir = tmp.path().join(".berth/runtime");
    std::fs::create_dir_all(&runtime_dir).unwrap();
    std::fs::write(runtime_dir.join("github.toml"), "not = [valid").unwrap();

    let status = berth_with_home(tmp.path())
        .args(["status"])
        .output()
        .unwrap();
    assert!(!status.status.success());
    let stdout = String::from_utf8_lossy(&status.stdout);
    assert!(stdout.contains("error"));
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
fn link_cursor_writes_config() {
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
        .args(["link", "cursor"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let config_path = tmp
        .path()
        .join(".berth/clients/cursor/cursor_mcp_config.json");
    assert!(config_path.exists());
    let content = std::fs::read_to_string(config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(
        json["mcpServers"]["github"]["env"]["GITHUB_TOKEN"],
        "abc123"
    );
}

#[test]
fn link_respects_env_permission_revoke() {
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
        .args(["permissions", "github", "--revoke", "env:GITHUB_TOKEN"])
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
    let content = std::fs::read_to_string(config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(json["mcpServers"]["github"]["env"]["GITHUB_TOKEN"].is_null());
}

#[test]
fn unlink_windsurf_removes_linked_servers() {
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
        .args(["link", "windsurf"])
        .output()
        .unwrap();

    let output = berth_with_home(tmp.path())
        .args(["unlink", "windsurf"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let config_path = tmp
        .path()
        .join(".berth/clients/windsurf/windsurf_mcp_config.json");
    let content = std::fs::read_to_string(config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(json["mcpServers"]["github"].is_null());
}

#[test]
fn link_unknown_client_exits_1() {
    let output = berth().args(["link", "unknown-client"]).output().unwrap();
    assert!(!output.status.success());
}

// --- permissions & audit ---

#[test]
fn permissions_show_declared_permissions() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();

    let output = berth_with_home(tmp.path())
        .args(["permissions", "github"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("network:api.github.com:443"));
    assert!(stdout.contains("env:GITHUB_TOKEN"));
}

#[test]
fn permissions_grant_and_revoke_persist_overrides() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();

    let grant = berth_with_home(tmp.path())
        .args([
            "permissions",
            "github",
            "--grant",
            "network:example.com:443",
        ])
        .output()
        .unwrap();
    assert!(grant.status.success());

    let show_after_grant = berth_with_home(tmp.path())
        .args(["permissions", "github"])
        .output()
        .unwrap();
    let stdout_grant = String::from_utf8_lossy(&show_after_grant.stdout);
    assert!(stdout_grant.contains("grant:"));
    assert!(stdout_grant.contains("network:example.com:443"));

    let revoke = berth_with_home(tmp.path())
        .args([
            "permissions",
            "github",
            "--revoke",
            "network:example.com:443",
        ])
        .output()
        .unwrap();
    assert!(revoke.status.success());

    let show_after_revoke = berth_with_home(tmp.path())
        .args(["permissions", "github"])
        .output()
        .unwrap();
    let stdout_revoke = String::from_utf8_lossy(&show_after_revoke.stdout);
    assert!(stdout_revoke.contains("revoke:"));
    assert!(stdout_revoke.contains("network:example.com:443"));
}

#[test]
fn permissions_reset_clears_overrides() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();

    berth_with_home(tmp.path())
        .args([
            "permissions",
            "github",
            "--grant",
            "network:example.com:443",
        ])
        .output()
        .unwrap();

    let reset = berth_with_home(tmp.path())
        .args(["permissions", "github", "--reset"])
        .output()
        .unwrap();
    assert!(reset.status.success());

    let show = berth_with_home(tmp.path())
        .args(["permissions", "github"])
        .output()
        .unwrap();
    assert!(show.status.success());
    let stdout = String::from_utf8_lossy(&show.stdout);
    assert!(!stdout.contains("network:example.com:443"));
}

#[test]
fn permissions_export_outputs_json() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();
    berth_with_home(tmp.path())
        .args([
            "permissions",
            "github",
            "--grant",
            "network:example.com:443",
        ])
        .output()
        .unwrap();

    let output = berth_with_home(tmp.path())
        .args(["permissions", "github", "--export"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(json["server"].as_str(), Some("github"));
    assert!(json["declared"]["network"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v.as_str() == Some("api.github.com:443")));
    assert!(json["overrides"]["grant"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v.as_str() == Some("network:example.com:443")));
    assert!(json["effective"]["network"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v.as_str() == Some("example.com:443")));
}

#[test]
fn permissions_rejects_invalid_format() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();

    let output = berth_with_home(tmp.path())
        .args(["permissions", "github", "--grant", "network:api.github.com"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Invalid permission format"));
}

#[test]
fn permissions_accepts_valid_env_permission_override() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();

    let output = berth_with_home(tmp.path())
        .args(["permissions", "github", "--grant", "env:GITHUB_TOKEN"])
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
fn audit_shows_runtime_events_for_server() {
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

    let output = berth_with_home(tmp.path())
        .args(["audit", "github"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("start"));
    assert!(stdout.contains("stop"));
    assert!(stdout.contains("github"));
}

#[test]
fn audit_invalid_since_exits_1() {
    let tmp = tempfile::tempdir().unwrap();
    let output = berth_with_home(tmp.path())
        .args(["audit", "--since", "bad"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

// --- proxy ---

#[test]
fn proxy_not_installed_exits_1() {
    let tmp = tempfile::tempdir().unwrap();
    let output = berth_with_home(tmp.path())
        .args(["proxy", "github"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn proxy_requires_config_before_running() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();

    let output = berth_with_home(tmp.path())
        .args(["proxy", "github"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Missing required config"));
}

#[test]
fn proxy_executes_child_and_records_audit() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();
    berth_with_home(tmp.path())
        .args(["config", "github", "--set", "token=abc123"])
        .output()
        .unwrap();
    patch_runtime_to_echo(tmp.path(), "github");

    let output = berth_with_home(tmp.path())
        .args(["proxy", "github"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("proxy-ok"));

    let audit = berth_with_home(tmp.path())
        .args(["audit", "github"])
        .output()
        .unwrap();
    assert!(audit.status.success());
    let audit_out = String::from_utf8_lossy(&audit.stdout);
    assert!(audit_out.contains("proxy-start"));
    assert!(audit_out.contains("proxy-end"));
}

#[test]
fn proxy_blocks_when_network_fully_revoked_and_audits() {
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
        .args(["permissions", "github", "--revoke", "network:*"])
        .output()
        .unwrap();
    patch_runtime_to_echo(tmp.path(), "github");

    let output = berth_with_home(tmp.path())
        .args(["proxy", "github"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Network permission denied"));

    let audit = berth_with_home(tmp.path())
        .args(["audit", "github"])
        .output()
        .unwrap();
    assert!(audit.status.success());
    let audit_out = String::from_utf8_lossy(&audit.stdout);
    assert!(audit_out.contains("permission-network-denied"));
}

#[test]
fn proxy_applies_env_permission_revoke() {
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
        .args(["permissions", "github", "--revoke", "env:GITHUB_TOKEN"])
        .output()
        .unwrap();
    patch_runtime_to_print_env_var(tmp.path(), "github", "GITHUB_TOKEN");

    let output = berth_with_home(tmp.path())
        .args(["proxy", "github"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("env-missing"));
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
