use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn berth() -> Command {
    Command::new(env!("CARGO_BIN_EXE_berth"))
}

fn berth_with_home(tmp: &std::path::Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_berth"));
    cmd.env("BERTH_HOME", tmp.join(".berth"));
    cmd
}

fn http_get(addr: &str, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).unwrap();
    let request = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).unwrap();

    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let first_line = response.lines().next().unwrap_or_default();
    let status = first_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, body)
}

fn http_post_json(addr: &str, path: &str, body: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).unwrap();
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(request.as_bytes()).unwrap();

    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let first_line = response.lines().next().unwrap_or_default();
    let status = first_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, body)
}

fn write_registry_override(path: &std::path::Path, servers: serde_json::Value) {
    let rendered = serde_json::to_string_pretty(&servers).unwrap();
    std::fs::write(path, rendered).unwrap();
}

fn write_publish_manifest(path: &std::path::Path) {
    let manifest = r#"
[server]
name = "acme-github"
display_name = "Acme GitHub MCP Server"
description = "GitHub integration for Acme"
version = "1.0.0"
category = "developer-tools"
maintainer = "Acme Inc"
trust_level = "community"

[source]
type = "npm"
package = "@acme/mcp-github"
repository = "https://github.com/acme/mcp-github"

[runtime]
type = "node"
command = "npx"
args = ["-y", "@acme/mcp-github"]
transport = "stdio"

[permissions]
network = ["api.github.com:443"]
env = ["GITHUB_TOKEN"]
filesystem = ["read:/workspace"]
exec = ["git"]

[compatibility]
clients = ["claude-desktop", "cursor"]
platforms = ["macos", "linux", "windows"]

[quality]
security_scan = "passed"
health_check = true
last_verified = "2026-02-21"
downloads = 0

[[config.required]]
key = "token"
env = "GITHUB_TOKEN"
description = "GitHub token"
sensitive = true

[[config.optional]]
key = "api_url"
description = "Override API URL"
default = "https://api.github.com"
"#;
    std::fs::write(path, manifest.trim_start()).unwrap();
}

fn write_github_raw_manifest(
    raw_base: &std::path::Path,
    repo: &str,
    git_ref: &str,
    manifest_rel_path: &str,
) -> std::path::PathBuf {
    let (owner, name) = repo.split_once('/').unwrap();
    let path = raw_base
        .join(owner)
        .join(name)
        .join(git_ref)
        .join(manifest_rel_path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    write_publish_manifest(&path);
    path
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

fn patch_runtime_to_fail_once_then_run(tmp: &std::path::Path, server: &str) {
    let config_path = tmp.join(".berth/servers").join(format!("{server}.toml"));
    let marker = tmp
        .join(".berth/runtime")
        .join(format!("{server}.restart-flag"));
    std::fs::create_dir_all(marker.parent().unwrap()).unwrap();

    let content = std::fs::read_to_string(&config_path).unwrap();
    let mut value: toml::Value = toml::from_str(&content).unwrap();
    let runtime = value
        .get_mut("runtime")
        .and_then(toml::Value::as_table_mut)
        .unwrap();

    #[cfg(unix)]
    {
        let script = format!(
            "if [ -f '{}' ]; then sleep 60; else touch '{}'; exit 1; fi",
            marker.display(),
            marker.display()
        );
        runtime.insert("command".to_string(), toml::Value::String("sh".to_string()));
        runtime.insert(
            "args".to_string(),
            toml::Value::Array(vec![
                toml::Value::String("-c".to_string()),
                toml::Value::String(script),
            ]),
        );
    }

    #[cfg(windows)]
    {
        let script = format!(
            "if exist \"{}\" (timeout /T 60 /NOBREAK >NUL) else (type nul > \"{}\" & exit /B 1)",
            marker.display(),
            marker.display()
        );
        runtime.insert(
            "command".to_string(),
            toml::Value::String("cmd".to_string()),
        );
        runtime.insert(
            "args".to_string(),
            toml::Value::Array(vec![
                toml::Value::String("/C".to_string()),
                toml::Value::String(script),
            ]),
        );
    }

    let rendered = toml::to_string_pretty(&value).unwrap();
    std::fs::write(&config_path, rendered).unwrap();
}

fn patch_runtime_to_fail_immediately(tmp: &std::path::Path, server: &str) {
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
                toml::Value::String("exit 1".to_string()),
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
                toml::Value::String("exit /B 1".to_string()),
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
fn readme_command_list_matches_cli_help_commands() {
    let output = berth().arg("--help").output().unwrap();
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);

    let mut help_commands = BTreeSet::new();
    let mut in_commands = false;
    for line in help.lines() {
        let trimmed = line.trim();
        if trimmed == "Commands:" {
            in_commands = true;
            continue;
        }
        if !in_commands {
            continue;
        }
        if trimmed.is_empty() {
            break;
        }
        if let Some(cmd) = trimmed.split_whitespace().next() {
            if cmd != "help" {
                help_commands.insert(cmd.to_string());
            }
        }
    }
    assert!(
        !help_commands.is_empty(),
        "No commands found in `berth --help`."
    );

    let readme_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../README.md");
    let readme = std::fs::read_to_string(&readme_path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", readme_path.display()));

    let commands_header = "## Commands";
    let section_start = readme
        .find(commands_header)
        .expect("README is missing `## Commands` header.");
    let section = &readme[section_start + commands_header.len()..];
    let fence_start = section
        .find("```")
        .expect("README `## Commands` section is missing opening code fence.");
    let after_fence = &section[fence_start + 3..];
    let fence_end = after_fence
        .find("```")
        .expect("README `## Commands` section is missing closing code fence.");
    let commands_block = &after_fence[..fence_end];

    let mut readme_commands = BTreeSet::new();
    for line in commands_block.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("berth ") {
            if let Some(cmd) = rest.split_whitespace().next() {
                if !cmd.is_empty() {
                    readme_commands.insert(cmd.to_string());
                }
            }
        }
    }
    assert!(
        !readme_commands.is_empty(),
        "No commands found in README command block."
    );

    let missing_in_readme: Vec<String> = help_commands
        .difference(&readme_commands)
        .cloned()
        .collect();
    let missing_in_help: Vec<String> = readme_commands
        .difference(&help_commands)
        .cloned()
        .collect();

    assert!(
        missing_in_readme.is_empty() && missing_in_help.is_empty(),
        "README command block is out of sync with `berth --help`.\nMissing in README: {:?}\nMissing in help: {:?}",
        missing_in_readme,
        missing_in_help
    );
}

#[test]
fn version_flag() {
    let output = berth().arg("--version").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("0.1.0"));
}

#[test]
fn registry_api_serves_health_search_and_downloads() {
    let tmp = tempfile::tempdir().unwrap();
    let mut child = berth_with_home(tmp.path())
        .args([
            "registry-api",
            "--bind",
            "127.0.0.1:0",
            "--max-requests",
            "8",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    let mut ready_line = String::new();
    {
        let stdout = child.stdout.as_mut().unwrap();
        let mut reader = BufReader::new(stdout);
        reader.read_line(&mut ready_line).unwrap();
    }
    assert!(ready_line.contains("http://"));
    let addr = ready_line
        .trim()
        .split("http://")
        .nth(1)
        .unwrap()
        .to_string();

    let (health_status, health_body) = http_get(&addr, "/health");
    assert_eq!(health_status, 200);
    let health: serde_json::Value = serde_json::from_str(&health_body).unwrap();
    assert_eq!(health["status"].as_str(), Some("ok"));

    let (search_status, search_body) = http_get(&addr, "/servers?q=github");
    assert_eq!(search_status, 200);
    let search: serde_json::Value = serde_json::from_str(&search_body).unwrap();
    assert!(search["count"].as_u64().unwrap_or(0) >= 1);

    let (filters_status, filters_body) = http_get(&addr, "/servers/filters");
    assert_eq!(filters_status, 200);
    let filters: serde_json::Value = serde_json::from_str(&filters_body).unwrap();
    assert!(filters["categories"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v.as_str() == Some("developer-tools")));

    let (filtered_status, filtered_body) = http_get(
        &addr,
        "/servers?category=developer-tools&platform=macos&trustLevel=official&limit=1",
    );
    assert_eq!(filtered_status, 200);
    let filtered: serde_json::Value = serde_json::from_str(&filtered_body).unwrap();
    assert_eq!(filtered["count"].as_u64(), Some(1));
    assert_eq!(
        filtered["servers"][0]["category"].as_str(),
        Some("developer-tools")
    );
    assert_eq!(
        filtered["servers"][0]["trustLevel"].as_str(),
        Some("official")
    );

    let (downloads_status, downloads_body) = http_get(&addr, "/servers/github/downloads");
    assert_eq!(downloads_status, 200);
    let downloads: serde_json::Value = serde_json::from_str(&downloads_body).unwrap();
    assert_eq!(downloads["server"].as_str(), Some("github"));
    assert_eq!(
        downloads["installCommand"].as_str(),
        Some("berth install github")
    );

    let (star_status, star_body) = http_post_json(&addr, "/servers/github/star", "{}");
    assert_eq!(star_status, 200);
    let star: serde_json::Value = serde_json::from_str(&star_body).unwrap();
    assert!(star["stars"].as_u64().unwrap_or(0) >= 1);

    let (report_status, report_body) = http_post_json(
        &addr,
        "/servers/github/report",
        "{\"reason\":\"spam\",\"details\":\"broken output\"}",
    );
    assert_eq!(report_status, 200);
    let report: serde_json::Value = serde_json::from_str(&report_body).unwrap();
    assert_eq!(report["status"].as_str(), Some("received"));

    let (community_status, community_body) = http_get(&addr, "/servers/github/community");
    assert_eq!(community_status, 200);
    let community: serde_json::Value = serde_json::from_str(&community_body).unwrap();
    assert!(community["stars"].as_u64().unwrap_or(0) >= 1);
    assert!(community["reports"].as_u64().unwrap_or(0) >= 1);

    let status = child.wait().unwrap();
    assert!(status.success());
}

#[test]
fn publish_dry_run_valid_manifest_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    write_publish_manifest(&tmp.path().join("berth.toml"));

    let output = berth_with_home(tmp.path())
        .current_dir(tmp.path())
        .args(["publish", "--dry-run"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("passed validation and quality checks"));
}

#[test]
fn publish_missing_manifest_exits_1() {
    let tmp = tempfile::tempdir().unwrap();

    let output = berth_with_home(tmp.path())
        .current_dir(tmp.path())
        .args(["publish", "--dry-run"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Failed to read manifest"));
}

#[test]
fn publish_invalid_manifest_exits_1() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest_path = tmp.path().join("berth.toml");
    write_publish_manifest(&manifest_path);
    let bad = std::fs::read_to_string(&manifest_path)
        .unwrap()
        .replace("transport = \"stdio\"", "transport = \"http\"")
        .replace("env = [\"GITHUB_TOKEN\"]", "env = [\"bad-var\"]");
    std::fs::write(&manifest_path, bad).unwrap();

    let output = berth_with_home(tmp.path())
        .current_dir(tmp.path())
        .args(["publish", "--dry-run"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Manifest validation failed"));
}

#[test]
fn publish_writes_submission_queue_entry() {
    let tmp = tempfile::tempdir().unwrap();
    write_publish_manifest(&tmp.path().join("berth.toml"));

    let output = berth_with_home(tmp.path())
        .current_dir(tmp.path())
        .args(["publish"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Submitted"));

    let queue_dir = tmp.path().join(".berth").join("publish").join("queue");
    let entries: Vec<_> = std::fs::read_dir(&queue_dir).unwrap().collect();
    assert_eq!(entries.len(), 1);

    let entry_path = entries[0].as_ref().unwrap().path();
    let payload = std::fs::read_to_string(entry_path).unwrap();
    let submission: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(
        submission["manifest"]["server"]["name"].as_str(),
        Some("acme-github")
    );
    assert_eq!(submission["status"].as_str(), Some("pending-manual-review"));
}

#[test]
fn import_github_dry_run_valid_manifest_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let raw_base = tmp.path().join("raw");
    write_github_raw_manifest(&raw_base, "acme/mcp-demo", "main", "berth.toml");

    let output = berth_with_home(tmp.path())
        .current_dir(tmp.path())
        .env(
            "BERTH_GITHUB_RAW_BASE",
            format!("file://{}", raw_base.display()),
        )
        .args(["import-github", "acme/mcp-demo", "--dry-run"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Valid GitHub manifest"));
    assert!(!tmp.path().join(".berth/servers/acme-github.toml").exists());
}

#[test]
fn import_github_writes_server_config() {
    let tmp = tempfile::tempdir().unwrap();
    let raw_base = tmp.path().join("raw");
    write_github_raw_manifest(&raw_base, "acme/mcp-demo", "main", "berth.toml");

    let output = berth_with_home(tmp.path())
        .current_dir(tmp.path())
        .env(
            "BERTH_GITHUB_RAW_BASE",
            format!("file://{}", raw_base.display()),
        )
        .args(["import-github", "https://github.com/acme/mcp-demo"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Imported"));

    let config_path = tmp.path().join(".berth/servers/acme-github.toml");
    assert!(config_path.exists());
    let config = std::fs::read_to_string(config_path).unwrap();
    assert!(config.contains("acme-github"));
    assert!(config.contains("https://github.com/acme/mcp-github"));
}

#[test]
fn import_github_invalid_manifest_exits_1() {
    let tmp = tempfile::tempdir().unwrap();
    let raw_base = tmp.path().join("raw");
    let path = write_github_raw_manifest(&raw_base, "acme/mcp-demo", "main", "berth.toml");
    let bad = std::fs::read_to_string(&path)
        .unwrap()
        .replace("transport = \"stdio\"", "transport = \"http\"")
        .replace("env = [\"GITHUB_TOKEN\"]", "env = [\"bad-var\"]");
    std::fs::write(path, bad).unwrap();

    let output = berth_with_home(tmp.path())
        .current_dir(tmp.path())
        .env(
            "BERTH_GITHUB_RAW_BASE",
            format!("file://{}", raw_base.display()),
        )
        .args(["import-github", "acme/mcp-demo"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Manifest validation failed"));
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

#[test]
fn install_python_runtime_server_uses_uvx() {
    let tmp = tempfile::tempdir().unwrap();
    let output = berth_with_home(tmp.path())
        .args(["install", "sequential-thinking"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let config_path = tmp.path().join(".berth/servers/sequential-thinking.toml");
    let content = std::fs::read_to_string(config_path).unwrap();
    let value: toml::Value = toml::from_str(&content).unwrap();
    let runtime = value
        .get("runtime")
        .and_then(toml::Value::as_table)
        .unwrap();
    assert_eq!(
        runtime.get("type").and_then(toml::Value::as_str),
        Some("python")
    );
    assert_eq!(
        runtime.get("command").and_then(toml::Value::as_str),
        Some("uvx")
    );
}

#[test]
fn install_binary_runtime_server_copies_local_artifact() {
    let tmp = tempfile::tempdir().unwrap();
    let source_binary = tmp.path().join("source-binary");
    std::fs::write(&source_binary, "#!/bin/sh\necho binary-demo\n").unwrap();

    let registry_file = tmp.path().join("registry.json");
    write_registry_override(
        &registry_file,
        serde_json::json!([
          {
            "name": "binary-demo",
            "displayName": "Binary Demo Server",
            "description": "Local binary runtime test server",
            "version": "0.1.0",
            "source": {
              "type": "binary",
              "package": source_binary.to_string_lossy(),
              "repository": "https://example.com/binary-demo"
            },
            "runtime": {
              "type": "binary",
              "command": "binary-demo",
              "args": []
            },
            "transport": "stdio",
            "permissions": {
              "network": [],
              "env": [],
              "filesystem": [],
              "exec": []
            },
            "config": {
              "required": [],
              "optional": []
            },
            "compatibility": {
              "clients": ["generic"],
              "platforms": ["linux", "macos", "windows"]
            },
            "quality": {
              "securityScan": "pass",
              "healthCheck": true,
              "lastVerified": "2026-02-21",
              "downloads": 1
            },
            "category": "developer-tools",
            "tags": ["binary", "test"],
            "maintainer": "Test",
            "trustLevel": "community"
          }
        ]),
    );

    let mut cmd = berth_with_home(tmp.path());
    cmd.env("BERTH_REGISTRY_INDEX_FILE", &registry_file);
    let output = cmd.args(["install", "binary-demo"]).output().unwrap();
    assert!(output.status.success());

    let binary_name = if cfg!(windows) {
        "binary-demo.exe"
    } else {
        "binary-demo"
    };
    let installed_binary = tmp.path().join(".berth/bin").join(binary_name);
    assert!(installed_binary.exists());

    let config_path = tmp.path().join(".berth/servers/binary-demo.toml");
    let content = std::fs::read_to_string(config_path).unwrap();
    let value: toml::Value = toml::from_str(&content).unwrap();
    let runtime = value
        .get("runtime")
        .and_then(toml::Value::as_table)
        .unwrap();
    let command = runtime
        .get("command")
        .and_then(toml::Value::as_str)
        .unwrap();
    assert_eq!(command, installed_binary.to_string_lossy());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(installed_binary)
            .unwrap()
            .permissions()
            .mode();
        assert_ne!(mode & 0o111, 0);
    }
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
fn config_set_secure_stores_secret_reference() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();

    let mut cmd = berth_with_home(tmp.path());
    cmd.env("BERTH_SECRET_BACKEND", "file");
    let output = cmd
        .args(["config", "github", "--set", "token=abc123", "--secure"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let config_path = tmp.path().join(".berth/servers/github.toml");
    let content = std::fs::read_to_string(config_path).unwrap();
    assert!(content.contains("secret://github/token"));
    assert!(!content.contains("abc123"));
}

#[test]
fn config_secure_without_set_exits_1() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();

    let output = berth_with_home(tmp.path())
        .args(["config", "github", "--secure"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn config_interactive_updates_value() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();

    let mut child = berth_with_home(tmp.path())
        .args(["config", "github", "--interactive"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"interactive-token\n\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    let config_path = tmp.path().join(".berth/servers/github.toml");
    let content = std::fs::read_to_string(config_path).unwrap();
    assert!(content.contains("interactive-token"));
}

#[test]
fn config_interactive_conflicts_with_set() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();

    let output = berth_with_home(tmp.path())
        .args(["config", "github", "--interactive", "--set", "token=abc123"])
        .output()
        .unwrap();
    assert!(!output.status.success());
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
fn config_set_runtime_policy_keys_updates_value() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();

    let out1 = berth_with_home(tmp.path())
        .args(["config", "github", "--set", "berth.auto-restart=true"])
        .output()
        .unwrap();
    assert!(out1.status.success());
    let out2 = berth_with_home(tmp.path())
        .args(["config", "github", "--set", "berth.max-restarts=2"])
        .output()
        .unwrap();
    assert!(out2.status.success());

    let config = std::fs::read_to_string(tmp.path().join(".berth/servers/github.toml")).unwrap();
    assert!(config.contains("\"berth.auto-restart\" = \"true\""));
    assert!(config.contains("\"berth.max-restarts\" = \"2\""));
}

#[test]
fn config_set_sandbox_policy_keys_updates_value() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();

    let out1 = berth_with_home(tmp.path())
        .args(["config", "github", "--set", "berth.sandbox=basic"])
        .output()
        .unwrap();
    assert!(out1.status.success());
    let out2 = berth_with_home(tmp.path())
        .args(["config", "github", "--set", "berth.sandbox-network=inherit"])
        .output()
        .unwrap();
    assert!(out2.status.success());

    let config = std::fs::read_to_string(tmp.path().join(".berth/servers/github.toml")).unwrap();
    assert!(config.contains("\"berth.sandbox\" = \"basic\""));
    assert!(config.contains("\"berth.sandbox-network\" = \"inherit\""));
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
fn start_blocks_when_sandbox_network_deny_all_and_audits() {
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
        .args(["config", "github", "--set", "berth.sandbox=basic"])
        .output()
        .unwrap();
    berth_with_home(tmp.path())
        .args([
            "config",
            "github",
            "--set",
            "berth.sandbox-network=deny-all",
        ])
        .output()
        .unwrap();

    let start = berth_with_home(tmp.path())
        .args(["start", "github"])
        .output()
        .unwrap();
    assert!(!start.status.success());
    let stderr = String::from_utf8_lossy(&start.stderr);
    assert!(stderr.contains("blocked by sandbox policy"));

    let audit = berth_with_home(tmp.path())
        .args(["audit", "github"])
        .output()
        .unwrap();
    assert!(audit.status.success());
    let audit_out = String::from_utf8_lossy(&audit.stdout);
    assert!(audit_out.contains("permission-network-denied"));
}

#[test]
fn proxy_sets_sandbox_env_when_basic_enabled() {
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
        .args(["config", "github", "--set", "berth.sandbox=basic"])
        .output()
        .unwrap();
    berth_with_home(tmp.path())
        .args(["config", "github", "--set", "berth.sandbox-network=inherit"])
        .output()
        .unwrap();
    patch_runtime_to_print_env_var(tmp.path(), "github", "BERTH_SANDBOX_MODE");

    let output = berth_with_home(tmp.path())
        .args(["proxy", "github"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("env-present"));
}

#[test]
fn start_warns_on_undeclared_network_grant_and_audits() {
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
        .args([
            "permissions",
            "github",
            "--grant",
            "network:example.com:443",
        ])
        .output()
        .unwrap();
    patch_runtime_to_long_running(tmp.path(), "github");

    let start = berth_with_home(tmp.path())
        .args(["start", "github"])
        .output()
        .unwrap();
    assert!(start.status.success());
    let stdout = String::from_utf8_lossy(&start.stdout);
    assert!(stdout.contains("undeclared network grant override"));

    let audit = berth_with_home(tmp.path())
        .args(["audit", "github"])
        .output()
        .unwrap();
    assert!(audit.status.success());
    let audit_out = String::from_utf8_lossy(&audit.stdout);
    assert!(audit_out.contains("permission-network-warning"));

    let stop = berth_with_home(tmp.path())
        .args(["stop", "github"])
        .output()
        .unwrap();
    assert!(stop.status.success());
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
fn status_auto_restart_recovers_crash_when_enabled() {
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
        .args(["config", "github", "--set", "berth.auto-restart=true"])
        .output()
        .unwrap();
    berth_with_home(tmp.path())
        .args(["config", "github", "--set", "berth.max-restarts=1"])
        .output()
        .unwrap();
    patch_runtime_to_fail_once_then_run(tmp.path(), "github");

    berth_with_home(tmp.path())
        .args(["start", "github"])
        .output()
        .unwrap();

    let mut running_seen = false;
    for _ in 0..80 {
        let status = berth_with_home(tmp.path())
            .args(["status"])
            .output()
            .unwrap();
        assert!(status.status.success());
        let stdout = String::from_utf8_lossy(&status.stdout);
        if stdout.contains("running") {
            running_seen = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(running_seen);

    let mut restart_seen = false;
    for _ in 0..80 {
        let audit = berth_with_home(tmp.path())
            .args(["audit", "github", "--action", "auto-restart"])
            .output()
            .unwrap();
        assert!(audit.status.success());
        let audit_out = String::from_utf8_lossy(&audit.stdout);
        if audit_out.contains("auto-restart") {
            restart_seen = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(restart_seen);
}

#[test]
fn auto_restart_happens_without_status_polling() {
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
        .args(["config", "github", "--set", "berth.auto-restart=true"])
        .output()
        .unwrap();
    berth_with_home(tmp.path())
        .args(["config", "github", "--set", "berth.max-restarts=1"])
        .output()
        .unwrap();
    patch_runtime_to_fail_once_then_run(tmp.path(), "github");

    let start = berth_with_home(tmp.path())
        .args(["start", "github"])
        .output()
        .unwrap();
    assert!(start.status.success());

    let mut saw_restart = false;
    for _ in 0..80 {
        let audit = berth_with_home(tmp.path())
            .args(["audit", "github", "--action", "auto-restart"])
            .output()
            .unwrap();
        if audit.status.success() && String::from_utf8_lossy(&audit.stdout).contains("auto-restart")
        {
            saw_restart = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(saw_restart);

    let stop = berth_with_home(tmp.path())
        .args(["stop", "github"])
        .output()
        .unwrap();
    assert!(stop.status.success());
}

#[test]
fn auto_restart_respects_max_restarts_setting() {
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
        .args(["config", "github", "--set", "berth.auto-restart=true"])
        .output()
        .unwrap();
    berth_with_home(tmp.path())
        .args(["config", "github", "--set", "berth.max-restarts=1"])
        .output()
        .unwrap();
    patch_runtime_to_fail_immediately(tmp.path(), "github");

    berth_with_home(tmp.path())
        .args(["start", "github"])
        .output()
        .unwrap();

    let mut count = 0usize;
    for _ in 0..80 {
        let audit = berth_with_home(tmp.path())
            .args(["audit", "github", "--action", "auto-restart"])
            .output()
            .unwrap();
        assert!(audit.status.success());
        let audit_out = String::from_utf8_lossy(&audit.stdout);
        count = audit_out
            .lines()
            .filter(|l| l.trim_start().starts_with("auto-restart"))
            .count();
        if count >= 1 {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(count, 1);
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
fn link_continue_writes_config() {
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
        .args(["link", "continue"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let config_path = tmp
        .path()
        .join(".berth/clients/continue/continue_config.json");
    assert!(config_path.exists());
    let content = std::fs::read_to_string(config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(
        json["mcpServers"]["github"]["env"]["GITHUB_TOKEN"],
        "abc123"
    );
}

#[test]
fn unlink_continue_removes_linked_servers() {
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
        .args(["link", "continue"])
        .output()
        .unwrap();

    let output = berth_with_home(tmp.path())
        .args(["unlink", "continue"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let config_path = tmp
        .path()
        .join(".berth/clients/continue/continue_config.json");
    let content = std::fs::read_to_string(config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(json["mcpServers"]["github"].is_null());
}

#[test]
fn link_vscode_writes_config() {
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
        .args(["link", "vscode"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let config_path = tmp
        .path()
        .join(".berth/clients/vscode/vscode_mcp_config.json");
    assert!(config_path.exists());
    let content = std::fs::read_to_string(config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(
        json["mcpServers"]["github"]["env"]["GITHUB_TOKEN"],
        "abc123"
    );
}

#[test]
fn unlink_vscode_removes_linked_servers() {
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
        .args(["link", "vscode"])
        .output()
        .unwrap();

    let output = berth_with_home(tmp.path())
        .args(["unlink", "vscode"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let config_path = tmp
        .path()
        .join(".berth/clients/vscode/vscode_mcp_config.json");
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
    assert!(stdout.contains("exec:git"));
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
    assert!(json["declared"]["exec"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v.as_str() == Some("git")));
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
fn permissions_accepts_valid_filesystem_and_exec_permission_override() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();

    let fs_output = berth_with_home(tmp.path())
        .args(["permissions", "github", "--grant", "filesystem:read:/tmp"])
        .output()
        .unwrap();
    assert!(fs_output.status.success());

    let exec_output = berth_with_home(tmp.path())
        .args(["permissions", "github", "--grant", "exec:git"])
        .output()
        .unwrap();
    assert!(exec_output.status.success());
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

#[test]
fn audit_action_filter_returns_only_matching_action() {
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
        .args(["audit", "github", "--action", "start"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("action=start"));
    assert!(stdout.contains("start"));
    assert!(!stdout.contains("stop"));
    assert!(stdout.contains("ago"));
}

#[test]
fn audit_json_output_is_machine_readable() {
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
        .args(["audit", "github", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let arr = json.as_array().unwrap();
    assert!(!arr.is_empty());
    assert!(arr.iter().any(|ev| ev["action"].as_str() == Some("start")));
    assert!(arr.iter().any(|ev| ev["action"].as_str() == Some("stop")));
}

#[test]
fn audit_export_json_output_writes_array_file() {
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

    let export_file = tmp.path().join("exports").join("audit.json");
    let output = berth_with_home(tmp.path())
        .args([
            "audit",
            "github",
            "--json",
            "--export",
            export_file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(export_file.exists());

    let content = std::fs::read_to_string(export_file).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    let arr = json.as_array().unwrap();
    assert!(!arr.is_empty());
    assert!(arr.iter().any(|ev| ev["action"].as_str() == Some("start")));
    assert!(arr.iter().any(|ev| ev["action"].as_str() == Some("stop")));
}

#[test]
fn audit_export_jsonl_output_writes_line_delimited_file() {
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

    let export_file = tmp.path().join("exports").join("audit.jsonl");
    let output = berth_with_home(tmp.path())
        .args([
            "audit",
            "github",
            "--action",
            "start",
            "--export",
            export_file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(export_file.exists());

    let content = std::fs::read_to_string(export_file).unwrap();
    let lines: Vec<&str> = content.lines().filter(|line| !line.is_empty()).collect();
    assert!(!lines.is_empty());
    for line in lines {
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(parsed["action"].as_str(), Some("start"));
        assert_eq!(parsed["server"].as_str(), Some("github"));
    }
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

#[test]
fn proxy_resolves_secure_secret_reference() {
    let tmp = tempfile::tempdir().unwrap();
    berth_with_home(tmp.path())
        .args(["install", "github"])
        .output()
        .unwrap();
    let mut secure = berth_with_home(tmp.path());
    secure.env("BERTH_SECRET_BACKEND", "file");
    let secure_set = secure
        .args(["config", "github", "--set", "token=abc123", "--secure"])
        .output()
        .unwrap();
    assert!(secure_set.status.success());

    patch_runtime_to_print_env_var(tmp.path(), "github", "GITHUB_TOKEN");

    let mut proxy = berth_with_home(tmp.path());
    proxy.env("BERTH_SECRET_BACKEND", "file");
    let output = proxy.args(["proxy", "github"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("env-present"));
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
