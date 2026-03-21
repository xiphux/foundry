mod cli;

use clap::Parser;
use cli::Cli;

fn main() {
    let cli = Cli::parse();

    match cli.command {
        cli::Commands::Start { name } => {
            println!("Starting workspace: {name}");
        }
        cli::Commands::Open { name } => {
            println!("Opening workspace: {}", name.unwrap_or_else(|| "(list)".into()));
        }
        cli::Commands::Finish { name } => {
            println!("Finishing workspace: {}", name.unwrap_or_else(|| "(infer)".into()));
        }
        cli::Commands::Discard { name } => {
            println!("Discarding workspace: {}", name.unwrap_or_else(|| "(infer)".into()));
        }
        cli::Commands::Projects(cmd) => match cmd {
            cli::ProjectsCommands::List => println!("Listing projects"),
            cli::ProjectsCommands::Add { name, path } => {
                println!("Adding project {name} at {}", path.display());
            }
            cli::ProjectsCommands::Remove { name } => {
                println!("Removing project {name}");
            }
        },
        cli::Commands::List => println!("Listing all workspaces"),
    }
}
