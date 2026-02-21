use colored::Colorize;
use std::process;

use berth_runtime::RuntimeManager;

use crate::paths;

pub fn execute(server: &str, tail: u32) {
    let config_path = match paths::server_config_path(server) {
        Some(p) => p,
        None => {
            eprintln!("{} Could not determine home directory.", "✗".red().bold());
            process::exit(1);
        }
    };

    if !config_path.exists() {
        eprintln!(
            "{} Server {} is not installed.",
            "✗".red().bold(),
            server.cyan()
        );
        process::exit(1);
    }

    let berth_home = match paths::berth_home() {
        Some(h) => h,
        None => {
            eprintln!("{} Could not determine home directory.", "✗".red().bold());
            process::exit(1);
        }
    };
    let runtime = RuntimeManager::new(berth_home);

    let lines = match runtime.tail_logs(server, tail as usize) {
        Ok(l) => l,
        Err(e) => {
            eprintln!(
                "{} Failed to read logs for {}: {}",
                "✗".red().bold(),
                server.cyan(),
                e
            );
            process::exit(1);
        }
    };

    if lines.is_empty() {
        println!(
            "{} No logs recorded for {} yet.",
            "!".yellow().bold(),
            server.cyan()
        );
        return;
    }

    println!(
        "{} Last {} log line(s) for {}:\n",
        "✓".green().bold(),
        lines.len(),
        server.cyan()
    );

    for line in lines {
        println!("  {}", line);
    }
}
