use clap::Parser;
use std::path::PathBuf;

/// Review AI-generated code with Git diffs, conversation history, and agent explanations.
///
/// wy is an interactive terminal app: run it inside a repository and press ? for help.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Repository to review (defaults to the current directory).
    #[arg(long, default_value = ".")]
    repo: PathBuf,
}

fn main() {
    let cli = Cli::parse();
    let result = wy::repository::root(&cli.repo).and_then(|root| wy::tui::run(&root));
    if let Err(error) = result {
        eprintln!("Error: {}", wy::security::redact(&format!("{error:#}")));
        std::process::exit(1);
    }
}
