use colored::Colorize;
use std::fs;
use std::path::PathBuf;

fn berth_servers_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".berth").join("servers"))
}

pub fn execute() {
    let servers_dir = match berth_servers_dir() {
        Some(d) => d,
        None => {
            eprintln!("{} Could not determine home directory.", "✗".red().bold());
            return;
        }
    };

    if !servers_dir.exists() {
        println!("{} No servers installed.\n", "!".yellow().bold());
        println!(
            "  Run {} to find servers, or {} to install one.",
            "berth search <query>".bold(),
            "berth install <server>".bold(),
        );
        return;
    }

    let entries: Vec<_> = match fs::read_dir(&servers_dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "toml")
                    .unwrap_or(false)
            })
            .collect(),
        Err(_) => {
            println!("{} No servers installed.\n", "!".yellow().bold());
            println!(
                "  Run {} to find servers, or {} to install one.",
                "berth search <query>".bold(),
                "berth install <server>".bold(),
            );
            return;
        }
    };

    if entries.is_empty() {
        println!("{} No servers installed.\n", "!".yellow().bold());
        println!(
            "  Run {} to find servers, or {} to install one.",
            "berth search <query>".bold(),
            "berth install <server>".bold(),
        );
        return;
    }

    println!(
        "{} {} server(s) installed:\n",
        "✓".green().bold(),
        entries.len()
    );

    println!("  {:<20} {:<12}", "NAME".bold(), "STATUS".bold(),);
    println!("  {}", "─".repeat(34));

    for entry in &entries {
        let name = entry
            .path()
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        // Status detection will come with the runtime; for now just show "stopped"
        println!("  {:<20} {:<12}", name.cyan(), "stopped".dimmed(),);
    }
    println!();
}
