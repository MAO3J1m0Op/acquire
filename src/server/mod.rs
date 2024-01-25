use std::borrow::Borrow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Serialize, Deserialize, Serializer};
use tokio::sync::{mpsc, broadcast, Notify};

use crate::game::tile::{Tile, FullHand};
use crate::game::{messages::*, Company};

use self::game::ServerGame;

mod game;

/// Spawns the tasks that manage the server. This function returns the channels
/// that the host will use to interface with the game. Closing either the sender
/// or receiver indicates that the host has quit and thus, the server will
/// close.
/// 
/// # Host Quitting Procedure
/// 
/// Dropping/closing the receiver will likely cause the host's client process to
/// exit, which will abort the program before the server has the time to
/// gracefully exit. Instead, drop the sender while continuing to listen to
/// messages on the receiver. This will give the server the time to exit. Once
/// the server is closed, the returned receiver will close.
// pub fn run() -> Server

/// Copyable handle to a running server.
#[derive(Debug, Clone)]
pub struct Server {
    /// Map of all the connections to the server.
    connections: Arc<Mutex<ConnectionManager>>,
    game: Arc<Mutex<ServerGame>>,
    /// Broadcaster that distributes messages from the server to where they need
    /// to go.
    broadcaster: broadcast::Sender<ServerBroadcast>,
    /// Channel used to send messages from the players to the server to be
    /// processed.
    client_sender: mpsc::Sender<TaggedClientMessage>,
}

impl Server {
    /// Starts a new server. This function returns two objects. First, the
    /// handle to the server. Second, the interface used by the host. If the
    /// host interface is closed, the server will gracefully shut down.
    pub fn start(
        max_players: Option<usize>,
        max_connections: Option<usize>,
        host_handshake: Handshake,
    ) -> (Self, NewConnection<std::convert::Infallible>) {

        let (broadcaster, _) = broadcast::channel(16);

        let mut connection_manager = ConnectionManager::new(
            max_players,
            max_connections
        );

        // Register the host as a player
        connection_manager.connect(host_handshake.clone()).unwrap();

        let interface_cm = connection_manager.clone();

        let connections = Arc::new(Mutex::new(connection_manager));
        let game = Arc::new(Mutex::new(ServerGame::new(broadcaster.clone())));

        let client_sender = Self::listen_for_requests(
            broadcaster.clone(),
            connections.clone(),
            game.clone(),
        );

        let server = Self {
            connections,
            game,
            broadcaster,
            client_sender,
        };    

        // Create the host interface
        let shutdown = Arc::new(Notify::new());
        let host_sender = server.player_to_server(
            host_handshake.clone(), shutdown.clone()
        );
        let host_recv = server.server_to_player(
            host_handshake.clone(), shutdown.clone()
        );

        // Broadcast a shutdown if the host quits
        let broadcaster = server.broadcaster.clone();
        tokio::spawn(async move {
            shutdown.notified().await;
            broadcaster.send(ServerBroadcast::Shutdown).ok();
        });

        let host_connection = NewConnection {
            handshake: host_handshake,
            server_state: ServerState {
                game_history: None,
                connections: interface_cm,
            },
            interface: Interface::new(host_sender, host_recv),
        };

        (server, host_connection)
    }

    /// Connects a player to the server, starting a process that transfer
    /// messages between the player and the server. Returns [`None`] if the
    /// player was not connected because the passed name was taken. 
    /// 
    /// # Disconnecting
    /// 
    /// To disconnect from the server, the client managing these channels should
    /// drop the sender first. The client should then continue to wait for
    /// messages on the receiver until the `recv` method returns [`None`]. This
    /// gives the server time to process the disconnection before the client
    /// exits. It is advised that the player not drop or close the returned
    /// receiver until it automatically closes.
    pub fn connect_player(&self, handshake: Handshake)
        -> Result<NewConnection<std::convert::Infallible>, ConnectionReject>
    {
        let mut connections_unlocked = self.connections.lock().unwrap();

        // Validate the connection
        connections_unlocked.connect(handshake.clone())?;

        // Broadcast a join message
        self.broadcaster.send(ServerBroadcast::Join {
            handshake: handshake.clone()
        }).unwrap();

        let shutdown = Arc::new(Notify::new());

        // Send messages from the client to the server
        let client_send = self.player_to_server(handshake.clone(), shutdown.clone());

        // Send messages from the server to the client
        let client_recv = self.server_to_player(handshake.clone(), shutdown.clone());

        let broadcaster = self.broadcaster.clone();
        let shutdown = shutdown.clone();
        let connections = self.connections.clone();
        let clone = handshake.clone();
        let name = handshake.player_name.clone();
        let disconnect_message = ServerBroadcast::Quit {
            handshake: clone
        };

        tokio::spawn(async move {

            // Task triggered on disconnect
            shutdown.notified().await;

            // Disconnect the player
            connections.lock().unwrap().disconnect(&name);

            // Send a disconnect message. Ignore any SendErrors, as an error
            // means that this is the last player to leave and the server will
            // shut down.
            broadcaster.send(disconnect_message).ok();
        });

        Ok(NewConnection {
            handshake,
            server_state: ServerState {
                game_history: self.game.lock().unwrap().history(),
                connections: connections_unlocked.clone(),
            },
            interface: Interface::new(client_send, client_recv),
        })
    }

    /// Starts one half of a player connection: forwards messages from the
    /// player to the server to be processed. Returns a sender to be part of an
    /// [`Interface`], as well as a shutdown listener to be used internally.
    fn player_to_server(&self,
        handshake: Handshake,
        shutdown: Arc<Notify>
    ) -> mpsc::Sender<ClientMessage> {

        // Clone necessary server parts
        let player_server_send = self.client_sender.clone();

        // Create the channels for this process
        let (client_send, mut client_player_recv) = mpsc::channel(1);

        tokio::spawn(async move {

            loop {
                let msg = tokio::select! {
                    msg = client_player_recv.recv() => msg,
                    _ = shutdown.notified() => {
                        dbg!("notify");
                        break
                    },
                };
                if let Some(msg) = msg {
                    player_server_send.send(TaggedClientMessage {
                        player_name: handshake.player_name.clone(),
                        kind: msg,
                    // Unwrap is ok because the server owns this receiving end
                    }).await.unwrap();
                } else {
                    dbg!("receiver close");
                    break;
                }
            }

            dbg!("player to server closed");
            shutdown.notify_waiters();
        });

        client_send
    }

    /// Starts one half of a player connection: provides messages from the
    /// server to the player. Returns a receiver to be part of an [`Interface`],
    /// and accepts a shutdown listener. The spawned task will notify the
    /// shutdown object if the receiver is closed or if it receives a shutdown
    /// message, and the task will shut down if it receives a notification.
    fn server_to_player(&self,
        handshake: Handshake,
        shutdown: Arc<Notify>,
    ) -> mpsc::Receiver<Result<ServerMessage, std::convert::Infallible>> {
        let mut broadcast_receiver = self.broadcaster.subscribe();

        // Receiver to be sent to the client
        let (player_client_send, client_recv) = mpsc::channel(1);

        tokio::spawn(async move {
            loop {
                let broadcast = tokio::select! {
                    msg_r = broadcast_receiver.recv() => match msg_r {
                        Ok(msg) => msg,
                        Err(why) => match why {
                            broadcast::error::RecvError::Closed => {
                                dbg!("close");
                                break
                            },
                            broadcast::error::RecvError::Lagged(_) => todo!(),
                        },
                    },
                    _ = shutdown.notified() => {
                        dbg!("notify");
                        break
                    }
                };
                
                let result = match broadcast {

                    // Handle the new tile of buying stock
                    ServerBroadcast::PlayerMove { action } => {
                        player_client_send.send(
                            Ok(ServerMessage::PlayerMove { action })
                        ).await
                    }

                    // Only forward the private message if it pertains to the
                    // player.
                    ServerBroadcast::Private {
                        target_player,
                        message
                    } => {
                        if &target_player == &handshake.player_name {
                            let msg = match message {
                                PrivateBroadcast::YourTurn { request } => {
                                    ServerMessage::YourTurn { request }
                                },
                                PrivateBroadcast::TileDraw { tile } => {
                                    ServerMessage::TileDraw { tile }
                                }
                                PrivateBroadcast::Invalid { reason } => {
                                    ServerMessage::Invalid { reason }
                                },
                            };
                            player_client_send.send(Ok(msg)).await
                        } else {
                            Ok(())
                        }
                    },

                    // If it receives a shutdown message, forward the message and exit
                    ServerBroadcast::Shutdown => {
                        // Ignore the send error, as we're shutting down anyway.
                        player_client_send.send(Ok(ServerMessage::Shutdown)).await.ok();
                        dbg!("shutdown");
                        break;
                    },

                    // Send only the part of the initial hand that pertains to the player
                    ServerBroadcast::GameStart { info, initial_hands } => {
                        let initial_hand = initial_hands
                            .get(&handshake.player_name)
                            .copied();
                        player_client_send.send(Ok(ServerMessage::GameStart {
                            info, initial_hand
                        })).await
                    }

                    ServerBroadcast::DeadTile { player_name, dead_tile } => {
                        player_client_send.send(
                            Ok(ServerMessage::DeadTile { player_name, dead_tile })
                        ).await
                    }

                    ServerBroadcast::Chat { player_name, message } => {
                        player_client_send.send(
                            Ok(ServerMessage::Chat { player_name, message })
                        ).await
                    },
                    ServerBroadcast::Join { handshake } => {
                        player_client_send.send(
                            Ok(ServerMessage::Join { handshake })
                        ).await
                    },
                    ServerBroadcast::CompanyDefunct { defunct, results } => {
                        player_client_send.send(
                            Ok(ServerMessage::CompanyDefunct { defunct, results })
                        ).await
                    },
                    ServerBroadcast::GameOver { reason, results } => {
                        player_client_send.send(
                            Ok(ServerMessage::GameOver { reason, results })
                        ).await
                    },
                    ServerBroadcast::Quit { handshake } => {
                        player_client_send.send(
                            Ok(ServerMessage::Quit { handshake })
                        ).await
                    }
                };

                // Stop the listener if the returned receiver was closed
                if result.is_err() {
                    dbg!("senderror");
                    break;
                }
            }
        
            dbg!("Server to player closed");
            shutdown.notify_waiters();
        });

        client_recv
    }

    /// Starts the process that responds to requests from clients. This involves
    /// controlling the game task and forwarding messages to it as necessary.
    /// Returns the sender used to send requests. This cannot be made into a
    /// function that takes `Self` as an argument, as the return value of this
    /// function is needed to construct a running [`Server`] object.
    fn listen_for_requests(
        broadcaster: broadcast::Sender<ServerBroadcast>,
        players: Arc<Mutex<ConnectionManager>>,
        game: Arc<Mutex<ServerGame>>,
    ) -> mpsc::Sender<TaggedClientMessage> {

        let (sender, mut receiver) = mpsc::channel::<TaggedClientMessage>(1);

        tokio::spawn(async move {

            while let Some(message) = receiver.recv().await {
                let mut game = game.lock().unwrap();
                match message.kind {
                    ClientMessage::TakingTurn(action) => {

                        let action = TaggedPlayerAction {
                            player_name: message.player_name.clone(),
                            action
                        };

                        game.update(action);
                    },
                    ClientMessage::Chat { message: chat_msg } => {
                        broadcaster.send(
                            ServerBroadcast::Chat {
                                player_name: message.player_name,
                                message: chat_msg,
                            }
                        ).unwrap();
                    },
                    ClientMessage::DeadTile { dead_tile } => {
                        game.swap_dead_tile(message.player_name, dead_tile);
                    },
                    ClientMessage::Admin(cmd) => {

                        // Check if the sender is an admin
                        let players = players.lock().unwrap();
                        let player = players.get_handshake(&message.player_name).unwrap();
                        if !player.admin {
                            broadcaster.send(ServerBroadcast::Private {
                                target_player: message.player_name,
                                message: PrivateBroadcast::Invalid {
                                    reason: InvalidMessageReason::PermissionDenied
                                }
                            }).unwrap();
                            continue;
                        }

                        match cmd {
                            AdminCommand::Shutdown => break,
                            AdminCommand::StartGame => {

                                // Determine which players aren't spectators.
                                let players: Vec<_> = players.players()
                                    .map(|s| s.to_owned().into_boxed_str())
                                    .collect();

                                game.start(6000, players, message.player_name);
                            },
                            AdminCommand::EndGame => {
                                game.end(message.player_name);
                            },
                            AdminCommand::Kick { player_name } => todo!(),
                            AdminCommand::SilenceChat => todo!(),
                        }
                    },
                };
            }

            // Send a shutdown message
            broadcaster.send(ServerBroadcast::Shutdown).unwrap();
        });

        sender
    }
}

/// Manages the players that are connected to the server.
#[derive(Debug, Clone)]
pub struct ConnectionManager {
    /// Maps the player's name to the remainder of the handshake, respectively
    /// whether the player is spectating and whether the player is an admin.
    connections: HashMap<Box<str>, (bool, bool)>,
    /// Number of players connected that aren't spectating.
    player_count: usize,
    /// Maximum number of players allowed to be connected to the server. Capped
    /// at 15. [`None`] if this instance isn't enforcing a cap.
    max_players: Option<usize>,
    /// Maximum total number of connections permitted on this server. [`None`]
    /// if this instance isn't enforcing a cap.
    max_connections: Option<usize>,
}

impl ConnectionManager {

    /// `max_players` must be a value between 1 and 15. If `max_players` is
    /// [`None`], the default value of 15 is used. `max_connections` can be any
    /// number. If [`None`] is passed, no limit will be placed on the number of
    /// connections.
    pub fn new(max_players: Option<usize>, max_connections: Option<usize>) -> Self {

        let max_players = max_players.unwrap_or(15);
        let max_connections = max_connections.unwrap_or(usize::MAX);

        assert!(max_players != 0, "max_players must not be zero");
        assert!(max_connections != 0, "max_connections must not be zero");
        assert!(max_players <= 15, 
            "max_players too high; expected 15, got {max_players}");

        Self {
            connections: HashMap::new(),
            player_count: 0,
            max_players: Some(max_players),
            max_connections: Some(max_connections),
        }
    }

    /// Creates a new [`ConnectionManager`] that places no limit on the number
    /// of connections. It's generally expected that the limit is enforced
    /// elsewhere in the code, i.e. server-side.
    pub fn new_limitless() -> Self {
        Self {
            connections: HashMap::new(),
            player_count: 0,
            max_players: None,
            max_connections: None,
        }
    }

    /// Get the number of players connected.
    pub fn player_count(&self) -> usize {
        self.player_count
    }

    /// Get the total number of connections, including spectators.
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// Gets the handshake of a connected player. If the player requested isn't
    /// connected, [`None`] will be returned.
    pub fn get_handshake(&self, name: &str) -> Option<Handshake> {
        let (spectating, admin) = *self.connections.get(name)?;

        Some(Handshake {
            player_name: name.to_owned().into_boxed_str(),
            spectating,
            admin,
        })
    }

    /// Gets an iterator over all of the players connected to the server.
    pub fn players(&self) -> impl Iterator<Item = &str> {
        self.connections()
            .filter(|(_, spectating)| !spectating)
            .map(|(name, _)| name.borrow())
    }

    /// Returns an iterator that iterates over all connections and provides the
    /// additional information of whether they're a spectator.
    pub fn connections(&self) -> impl Iterator<Item = (&str, bool)> {
        self.connections.iter()
            .map(|(name, (spectating, _admin))| (name.borrow(), *spectating))
    }

    pub fn handshakes(&self) -> impl Iterator<Item = Handshake> + '_ {
        self.connections.iter()
            .map(|(name, (spectating, admin))| {
                Handshake {
                    player_name: name.clone(),
                    spectating: *spectating,
                    admin: *admin
                }
            })
    }

    /// Connects a player to this connection manager.
    pub fn connect(&mut self, handshake: Handshake)
        -> Result<(), ConnectionReject>
    {
        use ConnectionReject::*;

        if self.connections.contains_key(&handshake.player_name) { return Err(NameTaken); }
        if let Some(max_connections) = self.max_connections {
            if self.connection_count() == max_connections { 
                return Err(MaxConnectionsReached);
            }
        }

        if !handshake.spectating {
            if let Some(max_players) = self.max_players {
                if self.player_count == max_players { return Err(FullGame); }
            }
        }

        self.connections.insert(handshake.player_name, (
            handshake.spectating,
            handshake.admin
        ));
        self.player_count += 1;

        Ok(())
    }

    /// Disconnects a player. Returns true if any action was needed.
    pub fn disconnect(&mut self, name: &str) -> bool {
        let data = self.connections.remove(name);
        if matches!(data, Some((false, _))) { self.player_count -= 1; }
        data.is_some()
    }
}

impl Serialize for ConnectionManager {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer
    {
        let handshakes: Vec<_> = self.handshakes().collect();
        handshakes.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ConnectionManager {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>
        {
            let vec: Vec<Handshake> = Deserialize::deserialize(deserializer)?;
            let mut manager = ConnectionManager::new_limitless();

            for c in vec {
                manager.connect(c).unwrap();
            }

            Ok(manager)
        }
}

/// The object used to introduce a player to the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Handshake {
    pub player_name: Box<str>,
    pub spectating: bool,
    /// Flag that indicates whether the player will be permitted to send admin
    /// commands.
    pub admin: bool,
}

/// Message sent from a client to the server.
#[derive(Debug, Clone)]
pub struct TaggedClientMessage {
    pub player_name: Box<str>,
    pub kind: ClientMessage,
}

/// A new connection to the server.
#[derive(Debug)]
pub struct NewConnection<E> {
    pub handshake: Handshake,
    pub server_state: ServerState,
    pub interface: Interface<E>,
}

/// An interface between the client and the server. This interface may be
/// through fallable means, so the type parameter permits the receiving of errors.
#[derive(Debug)]
#[must_use]
pub struct Interface<E> {
    sender: mpsc::Sender<ClientMessage>, 
    recv: mpsc::Receiver<Result<ServerMessage, E>>,
}

impl<E> Interface<E> {

    pub fn new(
        sender: mpsc::Sender<ClientMessage>, 
        recv: mpsc::Receiver<Result<ServerMessage, E>>,
    ) -> Self {
        Self { sender, recv }
    }

    /// Waits for a message from the server using the underlying receiver.
    pub async fn recv(&mut self) -> Option<Result<ServerMessage, E>> {
        self.recv.recv().await
    }

    /// Returns a reference to the underlying sender so that it can be cloned
    /// and distributed.
    pub fn sender(&self) -> &mpsc::Sender<ClientMessage> {
        &self.sender
    }

    /// Closes the contained sender of this interface, and then waits until the
    /// receiver is closed before returning. Calling this function, as opposed
    /// to simply dropping the interface, allows for the most graceful shutdown
    /// of the end that produced this interface.
    pub async fn close(mut self) -> Result<(), E> {
        std::mem::drop(self.sender);
        while let Some(result) = self.recv.recv().await {
            if let Err(e) = result { return Err(e); }
        }
        Ok(())
    }
}

/// Internal messages sent from the server loop to player handlers.
#[derive(Debug, Clone)]
pub enum ServerBroadcast {
    Chat {
        player_name: Box<str>,
        message: Box<str>,
    },
    Join {
        handshake: Handshake,
    },
    Quit {
        handshake: Handshake,
    },
    PlayerMove {
        action: TaggedPlayerAction,
    },
    DeadTile {
        player_name: Box<str>,
        dead_tile: Tile,
    },
    /// A new game has begun. This message is personalized for each player.
    GameStart {
        info: GameStart,
        initial_hands: HashMap<Box<str>, FullHand>
    },
    /// A company has gone defunct, and principle bonuses are to be paid out.
    CompanyDefunct {
        defunct: Company,
        results: Box<[PrincipleShareholderResult]>,
    },
    /// The game is over
    GameOver {
        reason: GameOver,
        results: Box<[FinalResult]>,
    },
    /// The server is shutting down.
    Shutdown,
    /// A message sent about a particular player that's meant only for the eyes
    /// of the targeted player.
    Private {
        target_player: Box<str>,
        message: PrivateBroadcast,
    },
}

#[derive(Debug, Clone)]
pub enum PrivateBroadcast {
    /// Tells a player it's their turn, and requests a specific game action.
    YourTurn {
        request: ActionRequest
    },
    /// The player drew a tile.
    TileDraw {
        tile: Tile
    },
    /// An invalid message was sent.
    Invalid {
        reason: InvalidMessageReason
    },
}

/// Indicates the current state of the server. This allows players to understand
/// what is going on after joining at any point in the game.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerState {
    pub game_history: Option<GameHistory>,
    pub connections: ConnectionManager,
}

/// Provides reasons for a [`Server`]'s rejection of a call to `connect_player`.
#[derive(Debug, thiserror::Error)]
pub enum ConnectionReject {
    /// The name picked is already in use. Connecting with a different name
    /// should work.
    #[error("name is already in use")]
    NameTaken,
    /// The game is full. Joining as a spectator should work.
    #[error("game is full")]
    FullGame,
    /// The server has reached its maximum number of connections and is
    /// therefore  not accepting any more.
    #[error("maximum connections reached")]
    MaxConnectionsReached
}
