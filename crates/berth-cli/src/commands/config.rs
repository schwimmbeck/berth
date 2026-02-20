use colored::Colorize;

pub fn execute(server: &str, set: Option<&str>, env: bool) {
    println!(
        "{} {} is not yet implemented.",
        "!".yellow().bold(),
        "berth config".bold()
    );
    println!("  Server: {}", server.cyan());
    if let Some(kv) = set {
        println!("  Would set: {}", kv);
    }
    if env {
        println!("  Would show required environment variables.");
    }
}
