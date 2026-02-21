//! Command handler for `berth audit`.

use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::fs;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::paths;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuditEvent {
    timestamp_epoch_secs: u64,
    server: String,
    action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    args: Option<Vec<String>>,
}

/// Executes the `berth audit` command.
pub fn execute(server: Option<&str>, since: Option<&str>, action: Option<&str>, json: bool) {
    let since_secs = match since {
        Some(raw) => match parse_since(raw) {
            Ok(v) => Some(v),
            Err(msg) => {
                eprintln!("{} {}", "✗".red().bold(), msg);
                process::exit(1);
            }
        },
        None => None,
    };

    let path = match paths::audit_log_path() {
        Some(p) => p,
        None => {
            eprintln!("{} Could not determine home directory.", "✗".red().bold());
            process::exit(1);
        }
    };

    if !path.exists() {
        println!("{} No audit entries yet.", "!".yellow().bold());
        return;
    }

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "{} Failed to read audit log {}: {}",
                "✗".red().bold(),
                path.display(),
                e
            );
            process::exit(1);
        }
    };

    let now = now_epoch_secs();
    let cutoff = since_secs.map(|s| now.saturating_sub(s));
    let mut events = Vec::new();
    let mut skipped = 0usize;

    for line in content.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<AuditEvent>(line) {
            Ok(ev) => {
                if let Some(name) = server {
                    if ev.server != name {
                        continue;
                    }
                }
                if let Some(filter_action) = action {
                    if ev.action != filter_action {
                        continue;
                    }
                }
                if let Some(c) = cutoff {
                    if ev.timestamp_epoch_secs < c {
                        continue;
                    }
                }
                events.push(ev);
            }
            Err(_) => skipped += 1,
        }
    }

    if events.is_empty() {
        if json {
            println!("[]");
            return;
        }
        println!("{} No matching audit entries.", "!".yellow().bold());
        return;
    }

    if json {
        let rendered = match serde_json::to_string_pretty(&events) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{} Failed to serialize audit JSON: {}", "✗".red().bold(), e);
                process::exit(1);
            }
        };
        println!("{rendered}");
        return;
    }

    println!(
        "{} Audit entries{}{}:\n",
        "✓".green().bold(),
        server
            .map(|s| format!(" for {}", s.cyan()))
            .unwrap_or_default(),
        action
            .map(|a| format!(" (action={})", a.bold()))
            .unwrap_or_default(),
    );

    println!(
        "  {:<24} {:<20} {:<22} {}",
        "ACTION".bold(),
        "SERVER".bold(),
        "TIME".bold(),
        "PID".bold()
    );
    println!("  {}", "─".repeat(80));
    for ev in &events {
        let pid = ev
            .pid
            .map(|p| p.to_string())
            .unwrap_or_else(|| "-".to_string());
        let ts = format_timestamp(ev.timestamp_epoch_secs, now);
        println!(
            "  {:<24} {:<20} {:<22} {}",
            ev.action.as_str(),
            ev.server.cyan(),
            ts,
            pid
        );
    }

    if skipped > 0 {
        println!(
            "\n{} Skipped {} malformed audit line(s).",
            "!".yellow().bold(),
            skipped
        );
    }
}

/// Parses `--since` strings like `30s`, `5m`, `1h`, `7d`.
fn parse_since(raw: &str) -> Result<u64, String> {
    if raw.len() < 2 {
        return Err("Invalid --since format. Use <number><s|m|h|d>.".to_string());
    }
    let (num, unit) = raw.split_at(raw.len() - 1);
    let n: u64 = num
        .parse()
        .map_err(|_| "Invalid --since number. Use <number><s|m|h|d>.".to_string())?;
    let mult = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        _ => {
            return Err("Invalid --since unit. Use s, m, h, or d.".to_string());
        }
    };
    Ok(n.saturating_mul(mult))
}

/// Returns current unix timestamp in seconds.
fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Formats epoch seconds with a relative-age suffix.
fn format_timestamp(epoch_secs: u64, now_epoch_secs: u64) -> String {
    let age = now_epoch_secs.saturating_sub(epoch_secs);
    format!("{epoch_secs} ({})", format_age(age))
}

/// Formats age in compact form, e.g. `12s ago`, `5m ago`.
fn format_age(seconds: u64) -> String {
    if seconds < 60 {
        return format!("{seconds}s ago");
    }
    if seconds < 3_600 {
        return format!("{}m ago", seconds / 60);
    }
    if seconds < 86_400 {
        return format!("{}h ago", seconds / 3_600);
    }
    format!("{}d ago", seconds / 86_400)
}
