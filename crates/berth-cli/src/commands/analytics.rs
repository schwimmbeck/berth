//! Command handler for `berth analytics`.

use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::paths;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuditEvent {
    timestamp_epoch_secs: u64,
    server: String,
    action: String,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct CountStat {
    value: String,
    count: u64,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct AnalyticsSummary {
    total_events: u64,
    unique_servers: u64,
    estimated_cost_usd: f64,
    earliest_event_epoch_secs: Option<u64>,
    latest_event_epoch_secs: Option<u64>,
    top_actions: Vec<CountStat>,
    top_servers: Vec<CountStat>,
}

/// Executes the `berth analytics` command.
pub fn execute(server: Option<&str>, since: Option<&str>, top: u32, json: bool) {
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
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&empty_summary(top as usize)).unwrap()
            );
        } else {
            println!("{} No audit entries yet.", "!".yellow().bold());
        }
        return;
    }

    let content = match fs::read_to_string(&path) {
        Ok(v) => v,
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

    let cutoff = since_secs.map(|seconds| now_epoch_secs().saturating_sub(seconds));
    let mut events = Vec::new();
    let mut skipped = 0usize;

    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        match serde_json::from_str::<AuditEvent>(line) {
            Ok(event) => {
                if let Some(server_name) = server {
                    if event.server != server_name {
                        continue;
                    }
                }
                if let Some(epoch_cutoff) = cutoff {
                    if event.timestamp_epoch_secs < epoch_cutoff {
                        continue;
                    }
                }
                events.push(event);
            }
            Err(_) => skipped += 1,
        }
    }

    let summary = summarize_events(&events, top as usize);
    if json {
        match serde_json::to_string_pretty(&summary) {
            Ok(rendered) => println!("{rendered}"),
            Err(e) => {
                eprintln!(
                    "{} Failed to serialize analytics JSON: {}",
                    "✗".red().bold(),
                    e
                );
                process::exit(1);
            }
        }
        return;
    }

    if summary.total_events == 0 {
        println!("{} No matching audit entries.", "!".yellow().bold());
        return;
    }

    println!(
        "{} Audit analytics{}{}:\n",
        "✓".green().bold(),
        server
            .map(|name| format!(" for {}", name.cyan()))
            .unwrap_or_default(),
        since
            .map(|window| format!(" (since {})", window.bold()))
            .unwrap_or_default()
    );
    println!(
        "  total events: {}   unique servers: {}   estimated cost: ${:.4}",
        summary.total_events.to_string().bold(),
        summary.unique_servers.to_string().bold(),
        summary.estimated_cost_usd
    );
    if let (Some(first), Some(last)) = (
        summary.earliest_event_epoch_secs,
        summary.latest_event_epoch_secs,
    ) {
        println!("  time range: {first} .. {last}");
    }

    println!("\n  {}", "Top actions".bold());
    print_count_stats(&summary.top_actions);
    println!("\n  {}", "Top servers".bold());
    print_count_stats(&summary.top_servers);

    if skipped > 0 {
        println!(
            "\n{} Skipped {} malformed audit line(s).",
            "!".yellow().bold(),
            skipped
        );
    }
}

fn empty_summary(top: usize) -> AnalyticsSummary {
    let _ = top;
    AnalyticsSummary {
        total_events: 0,
        unique_servers: 0,
        estimated_cost_usd: 0.0,
        earliest_event_epoch_secs: None,
        latest_event_epoch_secs: None,
        top_actions: Vec::new(),
        top_servers: Vec::new(),
    }
}

fn summarize_events(events: &[AuditEvent], top: usize) -> AnalyticsSummary {
    if events.is_empty() {
        return empty_summary(top);
    }

    let mut action_counts = BTreeMap::<String, u64>::new();
    let mut server_counts = BTreeMap::<String, u64>::new();
    let mut servers = BTreeSet::<String>::new();
    let mut estimated_cost_usd = 0.0_f64;
    let mut earliest = u64::MAX;
    let mut latest = 0_u64;

    for event in events {
        *action_counts.entry(event.action.clone()).or_insert(0) += 1;
        *server_counts.entry(event.server.clone()).or_insert(0) += 1;
        servers.insert(event.server.clone());

        earliest = earliest.min(event.timestamp_epoch_secs);
        latest = latest.max(event.timestamp_epoch_secs);
        estimated_cost_usd += action_cost_estimate_usd(&event.action);
    }

    let top_actions = top_counts(&action_counts, top);
    let top_servers = top_counts(&server_counts, top);

    AnalyticsSummary {
        total_events: events.len() as u64,
        unique_servers: servers.len() as u64,
        estimated_cost_usd,
        earliest_event_epoch_secs: Some(earliest),
        latest_event_epoch_secs: Some(latest),
        top_actions,
        top_servers,
    }
}

fn top_counts(map: &BTreeMap<String, u64>, top: usize) -> Vec<CountStat> {
    let mut entries = map
        .iter()
        .map(|(value, count)| CountStat {
            value: value.clone(),
            count: *count,
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.value.cmp(&right.value))
    });
    entries.into_iter().take(top).collect()
}

fn action_cost_estimate_usd(action: &str) -> f64 {
    match action {
        "proxy-start" | "proxy-end" | "proxy-error" => 0.0020,
        "start" | "stop" | "restart" | "exit" | "auto-restart" => 0.0005,
        _ => 0.0,
    }
}

fn print_count_stats(stats: &[CountStat]) {
    if stats.is_empty() {
        println!("    (none)");
        return;
    }
    println!("    {:<28} {}", "VALUE".bold(), "COUNT".bold());
    println!("    {}", "─".repeat(40));
    for stat in stats {
        println!("    {:<28} {}", stat.value, stat.count);
    }
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Parses `--since` values like `30s`, `10m`, `24h`, `7d`.
fn parse_since(raw: &str) -> Result<u64, String> {
    if raw.len() < 2 {
        return Err("Invalid --since format. Use <number><s|m|h|d>.".to_string());
    }

    let (num, unit) = raw.split_at(raw.len() - 1);
    let value: u64 = num
        .parse()
        .map_err(|_| "Invalid --since number. Use <number><s|m|h|d>.".to_string())?;
    let multiplier = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        _ => return Err("Invalid --since unit. Use s, m, h, or d.".to_string()),
    };
    Ok(value.saturating_mul(multiplier))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(ts: u64, server: &str, action: &str) -> AuditEvent {
        AuditEvent {
            timestamp_epoch_secs: ts,
            server: server.to_string(),
            action: action.to_string(),
        }
    }

    #[test]
    fn parse_since_accepts_valid_values() {
        assert_eq!(parse_since("30s").unwrap(), 30);
        assert_eq!(parse_since("5m").unwrap(), 300);
        assert_eq!(parse_since("2h").unwrap(), 7_200);
        assert_eq!(parse_since("1d").unwrap(), 86_400);
    }

    #[test]
    fn parse_since_rejects_invalid_values() {
        assert!(parse_since("bad").is_err());
        assert!(parse_since("10").is_err());
        assert!(parse_since("1w").is_err());
    }

    #[test]
    fn summarize_events_aggregates_counts_and_cost() {
        let events = vec![
            ev(100, "github", "start"),
            ev(110, "github", "stop"),
            ev(115, "github", "proxy-start"),
            ev(120, "filesystem", "start"),
        ];
        let summary = summarize_events(&events, 5);

        assert_eq!(summary.total_events, 4);
        assert_eq!(summary.unique_servers, 2);
        assert_eq!(summary.earliest_event_epoch_secs, Some(100));
        assert_eq!(summary.latest_event_epoch_secs, Some(120));
        assert!((summary.estimated_cost_usd - 0.0035).abs() < 0.00001);
        assert_eq!(
            summary.top_servers[0],
            CountStat {
                value: "github".to_string(),
                count: 3
            }
        );
        assert_eq!(
            summary
                .top_actions
                .iter()
                .map(|item| item.count)
                .sum::<u64>(),
            4
        );
    }
}
