use std::io;
use std::net::Ipv4Addr;

use clap::Parser;
use server::{Server, Handshake};
use tokio::net::{TcpListener, TcpStream};

mod cli;
mod client;
mod game;
mod net;
mod server;

#[tokio::main]
async fn main() {

    let cli = cli::Cli::parse();

    let host_handshake = Handshake {
        player_name: cli.name.into_boxed_str(),
        spectating: cli.spectate,
        admin: false,
    };

    let result = match cli.intent {
        cli::HostIntent::Join { address } => {
            join(address, host_handshake).await
        },
        cli::HostIntent::Host { port } => {
            host(port, host_handshake).await
        },
    };

    if let Err(why) = result {
        eprintln!("Connection failed: {why}");
    }
}

/// Hosts a game
async fn join(address: String, handshake: Handshake) -> io::Result<()> {
    let socket = TcpStream::connect(address).await?;
    println!("Connected to remote server.");

    let remote_connection = net::RemoteServer::new(socket)
        .connect_player(handshake).await?;
    println!("Successfully joined server! Starting client.");

    client::robust::run_io(remote_connection).await?;
    Ok(())
}

async fn host(port: u16, mut handshake: Handshake) -> io::Result<()> {

    // Set the handshake's admin to true, since the host is an administrator
    handshake.admin = true;

    // Start the server
    let (server, host_interface) = Server::start(
        Some(8), Some(16), handshake
    );

    // Start the TCP listener
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
    println!("Server started: listening at {}.", listener.local_addr()?);
    let net_handle = tokio::spawn(net::listen_for_connections(server, listener));

    // Start the client
    println!("Starting client");
    std::thread::sleep(std::time::Duration::from_secs(1));
    match client::robust::run(host_interface).await? {
        Ok(()) => {},
        Err(_) => {},
    };

    Ok(())
}
