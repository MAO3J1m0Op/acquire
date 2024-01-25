use std::io;

use tokio::io::{BufStream, AsyncBufReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

use crate::game::messages::*;
use crate::server::{Server, Interface, Handshake, ServerState, NewConnection};

const DELIM: u8 = '\n' as u8;

/// Listens for incoming TCP connections on the passed listener and connects
/// them to a server.
pub async fn listen_for_connections(
    server: Server,
    stream: TcpListener,
) -> io::Result<()> {

    let server = server.clone();

    loop {
        let (socket, _addr) = stream.accept().await?;
        let mut socket = RemoteClient::new(socket);

        // Start the connection, but in a separate task to not block the accept loop
        let server_clone = server.clone();
        tokio::spawn(async move {

            // Check if the handshake is valid
            let connection = recv_and_connect(&server_clone, &mut socket).await;

            let interface = match connection {
                Ok(Some(interface)) => interface,
                Err(_) | Ok(None) => {
                    // Shut down the socket; ignore any errors, as we have no
                    // way to report them
                    socket.0.shutdown().await.ok();

                    // Terminate this connection
                    return;
                },
            };

            // Ignore any IO errors, as we have no way to report them
            socket.connect(interface).await.ok();
        });
    }
}

/// Tries to receive a handshake through the socket and connect it to the
/// provided [`Server`]. Responds to the sender with the proper response, be it
/// a success message or an error message. If the handshake was invalid,
/// `Ok(None)` is returned, and if a valid handshake is received,
/// `Ok(Some(...))` is returned.
async fn recv_and_connect(
    server: &Server,
    socket: &mut RemoteClient,
) -> io::Result<Option<NewConnection<std::convert::Infallible>>> {

    // Get the handshake
    let mut handshake_buf = Vec::new();
    let bytes_read = socket.0.read_until(DELIM, &mut handshake_buf).await?;

    if bytes_read == 0 {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof,
            "Unexpected EOF when sending handshake"
        ));
    }

    // Parse the handshake
    let interface = serde_json::from_slice(&handshake_buf)
        .map_err(|e| e.to_string())

        // Reject any connections requesting admin privileges
        .and_then(|handshake: Handshake| {
            if handshake.admin {
                Err("Cannot join as administrator".to_owned())
            } else {
                Ok(handshake)
            }
        })

        // Try to connect the handshake
        .and_then(|handshake: Handshake| {
            server.connect_player(handshake)
                .map_err(|e| e.to_string())
        });

    // Write the response to the socket
    Ok(match interface {
        Ok(interface) => {
            socket.0.write_all(HANDSHAKE_ACCEPT).await?;
            socket.0.flush().await?;
            Some(interface)
        },
        Err(reject_msg) => {
            socket.0.write_all(HANDSHAKE_REJECT_NEEDLE).await?;
            socket.0.write_all(reject_msg.as_bytes()).await?;
            socket.0.write_all(&[DELIM]).await?;
            socket.0.flush().await?;
            None
        },
    })
}

/// Message sent to indicate that a handshake was accepted and the server has
/// opened the connection.
const HANDSHAKE_ACCEPT: &[u8] = b"Accepted\n";
/// Beginning of a message sent to indicate that a handshake was rejected for
/// some reason.
const HANDSHAKE_REJECT_NEEDLE: &[u8] = b"Rejected: ";

/// Remote proxy for a client.
pub struct RemoteClient(BufStream<TcpStream>);

impl RemoteClient {
    pub fn new(socket: TcpStream) -> Self {
        Self(BufStream::new(socket))
    }

    /// Connects any fallible interface. Upon receiving of an error, terminates
    /// the connection and returns the error.
    pub async fn connect<E>(mut self, connection: NewConnection<E>)
        -> io::Result<Result<(), E>>
    {
        // Initiate the connection by sending the server's state
        let state = serde_json::to_vec(&connection.server_state).unwrap();
        self.0.write_all(&state).await?;
        self.0.write_all(&[DELIM]).await?;
        self.0.flush().await?;

        let mut incoming_msg_buffer = Vec::new();
        let mut interface = connection.interface;
        loop {
            tokio::select! {
                // Serialize outgoing messages
                msg = interface.recv() => {

                    if let Some(msg_result) = msg {
                        
                        let msg = match msg_result {
                            Ok(v) => v,
                            Err(e) => return Ok(Err(e)),
                        };

                        self.send_message(&msg).await?;
                    } else {
                        break;
                    }
                },
                // Deserialize incoming messages
                bytes_res = self.0.read_until(DELIM, &mut incoming_msg_buffer) => {
                    let bytes = bytes_res?;

                    // TODO: error on messages that are too long

                    // Shut down if we get an EOF
                    if bytes == 0 { break; }

                    // Deserialize the message
                    let msg_result = serde_json::from_slice(&incoming_msg_buffer);

                    // Create the error message
                    let msg_result = match msg_result {
                        Err(e) => Err(ServerMessage::Invalid {
                            reason: InvalidMessageReason::JsonParseErr(
                                e.to_string().into_boxed_str()
                            )
                        }),
                        Ok(v) => Ok(v),
                    };

                    // Clear the message buffer since we've gotten the whole message
                    incoming_msg_buffer.clear();

                    // Either forward the message to the client or return the error
                    match msg_result {
                        Ok(msg) => interface.sender().send(msg).await.unwrap(),
                        Err(err) => self.send_message(&err).await?,
                    };
                }
            }
        }

        // Cleanly shut down
        self.0.shutdown().await?;
        if let Err(e) = interface.close().await {
            return Ok(Err(e))
        }

        Ok(Ok(()))
    }

    async fn send_message(&mut self, msg: &ServerMessage) -> io::Result<()> {
        // There shouldn't be an error on serializing
        let msg_ser = serde_json::to_vec(&msg).unwrap();

        self.0.write_all(&msg_ser).await?;
        self.0.write_all(&[DELIM]).await?;
        self.0.flush().await?;
        Ok(())
    }
}

/// Remote proxy for a server. Capable of connecting one player.
#[derive(Debug)]
pub struct RemoteServer(BufStream<TcpStream>);

impl RemoteServer {

    pub fn new(socket: TcpStream) -> Self {
        Self(BufStream::new(socket))
    }

    /// Tries to send a handshake, then waits for an acknowledgment from the
    /// server. Returns [`Ok`] with the state of the server if the handshake was
    /// accepted, and an error with the rejection message if the handshake was
    /// rejected.
    async fn send_handshake(&mut self, handshake: &Handshake)
        -> io::Result<ServerState>
    {
        let handshake_msg = serde_json::to_vec(&handshake).unwrap();
        self.0.write_all(&handshake_msg).await?;
        self.0.write_all(&[DELIM]).await?;
        self.0.flush().await?;

        // Wait for a reply from the server.
        let mut reply_buf = vec![];
        let bytes_read = self.0.read_until(DELIM, &mut reply_buf).await?;
        if bytes_read == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof,
                "Unexpected EOF when sending handshake"
            ));
        };

        if &reply_buf == HANDSHAKE_ACCEPT {

            // Wait for the server to send its state
            let mut reply_buf = vec![];
            let bytes_read = self.0.read_until(DELIM, &mut reply_buf).await?;
            if bytes_read == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof,
                    "Unexpected EOF when sending handshake"
                ));
            };

            // TODO: handle a bad message from the server
            let server_state: ServerState = serde_json::from_slice(&reply_buf).unwrap();

            return Ok(server_state);
        }

        else if reply_buf.starts_with(HANDSHAKE_REJECT_NEEDLE) {
            
            // Get the rejection message
            let start = HANDSHAKE_REJECT_NEEDLE.len();
            let message = std::str::from_utf8(&reply_buf[start..])
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?
                .to_owned();
            return Err(io::Error::new(io::ErrorKind::ConnectionRefused, message));
        }

        return Err(io::Error::new(io::ErrorKind::InvalidData,
            "Server sent improper reply to handshake"
        ));
    }

    /// Serializes and sends a [`ClientMessage`] to the server.
    async fn send_message(&mut self, msg: &ClientMessage) -> io::Result<()> {

        // There shouldn't be an error on serializing
        let msg_ser = serde_json::to_vec(&msg).unwrap();
        self.0.write_all(&msg_ser).await?;
        self.0.write_all(&[DELIM]).await?;
        self.0.flush().await?;
        Ok(())
    }
    
    /// Creates a player [`Interface`] that interfaces with this remote server.
    pub async fn connect_player(mut self, handshake: Handshake)
        -> io::Result<NewConnection<io::Error>>
    {
        // Try to connect to the server
        let server_state = self.send_handshake(&handshake).await?;

        let (
            interface_sender,
            mut outgoing_recv
        ) = mpsc::channel::<ClientMessage>(1);
        let (
            outgoing_sender,
            interface_recv
        ) = mpsc::channel(1);
        tokio::spawn(async move {
            let mut incoming_msg_buffer = Vec::new();

            loop {
                tokio::select! {
                    // Serialize outgoing messages
                    msg = outgoing_recv.recv() => {

                        if let Some(msg) = msg {
                            if let Err(why) = self.send_message(&msg).await {
                                // If we discover that the receiver is dropped, shut down
                                if let Err(_) = outgoing_sender.send(Err(why)).await {
                                    break;
                                }
                            }
                        // The interface dropped its sender, meaning it's time to shut down
                        } else {
                            break;
                        }
                    },
                    // Deserialize incoming messages
                    bytes_res = self.0.read_until(DELIM, &mut incoming_msg_buffer) => {

                        let bytes = match bytes_res {
                            Ok(v) => v,
                            Err(e) => {
                                // Send the error, if anyone's listening
                                outgoing_sender.send(Err(e)).await.ok();
                                break;
                            },
                        };

                        // TODO: error on messages that are too long

                        // Shut down if we get an EOF
                        if bytes == 0 { break; }

                        // Deserialize the message. Messages from the server
                        // should be valid JSON, so we can call unwrap.
                        let msg = serde_json::from_slice(&incoming_msg_buffer).unwrap();

                        // Clear the message buffer since we've gotten the whole message
                        incoming_msg_buffer.clear();

                        // Send the result
                        if let Err(_) = outgoing_sender.send(Ok(msg)).await {
                            // Shut down if the interface is dropped
                            break;
                        }
                    }
                }
            }

            // Cleanly shut down
            let shutdown_result = self.0.shutdown().await;
            if let Err(why) = shutdown_result {
                outgoing_sender.send(Err(why)).await.ok();
            }
        });

        Ok(NewConnection {
            handshake,
            server_state,
            interface: Interface::new(interface_sender, interface_recv),
        })
    }
}
