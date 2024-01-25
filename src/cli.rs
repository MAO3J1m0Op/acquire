use clap::{Parser, Subcommand};

/// Command-line interface for the legendary board game Acquire!
#[derive(Debug, Parser)]
pub struct Cli {
    #[command(subcommand)]
    pub intent: HostIntent,
    /// User name used when connecting to the server
    #[arg(short, long)]
    pub name: String,
    /// If set, you will join the game as a spectator
    #[arg(short, long)]
    pub spectate: bool,
}

#[derive(Debug, Subcommand)]
pub enum HostIntent {
    /// Join a game hosted elsewhere
    Join {
        /// IP address to join
        address: String,
    },
    /// Host a game on your machine
    Host {
        /// Port to which other players will connect to join
        port: u16,
    }
}
