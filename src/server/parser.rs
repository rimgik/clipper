use std::net::SocketAddr;

pub use clap::Parser;

/// Clipper server
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    /// Target socket
    #[arg(short, long)]
    pub socket: SocketAddr,
}
