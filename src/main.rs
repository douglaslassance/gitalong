//! Binary entry point. Parses the CLI and dispatches to [`gitalong::commands`].

use anyhow::Result;
use clap::Parser;

use gitalong::cli::{Cli, Command};
use gitalong::commands::{self, GlobalOpts};

fn main() -> Result<()> {
    let cli = Cli::parse();
    let opts = GlobalOpts::resolve(cli.repository, cli.git_binary)?;

    match cli.command {
        Command::Version => commands::version(&opts),
        Command::Config { property } => commands::config(&opts, &property),
        Command::Setup(args) => commands::setup(&opts, args),
        Command::Update { profile } => commands::update(&opts, profile),
        Command::Status { files, profile } => commands::status(&opts, &files, profile),
        Command::Claim { files, profile } => commands::claim(&opts, &files, profile),
    }
}
