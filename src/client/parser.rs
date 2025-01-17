use std::net::SocketAddr;

pub use clap::Parser;

/// Clipper client
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    /// Target socket
    #[arg(short, long)]
    pub socket: SocketAddr,
    /// Use encryption
    #[arg(short, long)]
    pub encrypted: bool,
    /// Verbose
    #[arg(short, long)]
    pub verbose: bool,
}
