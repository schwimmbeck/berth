use colored::Colorize;

pub fn execute(server: Option<&str>, since: Option<&str>) {
    println!(
        "{} {} is not yet implemented.",
        "!".yellow().bold(),
        "berth audit".bold()
    );
    if let Some(name) = server {
        println!("  Would show audit log for server: {}", name.cyan());
    } else {
        println!("  Would show audit log for all servers.");
    }
    if let Some(duration) = since {
        println!("  Since: {}", duration);
    }
}
