//! Writes the `git-along` man pages into the directory given as the only argument.

use clap::CommandFactory;

fn main() -> std::io::Result<()> {
    let dir = std::env::args()
        .nth(1)
        .expect("usage: cargo run --example man -- <dir>");
    std::fs::create_dir_all(&dir)?;
    clap_mangen::generate_to(git_along::cli::Cli::command(), dir)
}
