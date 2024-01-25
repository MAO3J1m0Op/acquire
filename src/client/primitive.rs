use std::sync::{Arc, Mutex};

use tokio::sync::{mpsc, oneshot};

use super::{CommandParseErr, parse_game_command, parse_admin_command, ClientGame};
use crate::game::{tile::Tile, kernel::Game, messages::*};
use crate::server::{Interface, Handshake, NewConnection};

/// Runs this client. This client is "owned" by the receiver, meaning it will
/// run until the receiver is dropped or closed.
pub async fn run<E: Send + 'static>(mut connection: NewConnection<E>) -> Result<(), E> {
    // Game objects passed to the two processes
    let game = Arc::new(Mutex::new(
        ClientGame::new(
            connection.handshake.clone(),
            connection.server_state.game_history.take()
        )));
    let game_copy = Arc::clone(&game);
    
    // Exit handlers
    let (exit_sender, exit_recv) = oneshot::channel();

    let player_name_clone = connection.handshake.player_name
        .to_string()
        .into_boxed_str();
    let sender = connection.interface.sender().clone();
    let event_loop = tokio::spawn(async move {

        // Run the loop
        event_loop(game_copy, connection.handshake, &mut connection.interface).await?;

        // Once the event loop exits, notify the io loop. Unwrap works here, as
        // the exit receiver should never close.
        exit_sender.send(()).unwrap();

        connection.interface.close().await?;

        Ok(())
    });

    let io_loop = tokio::task::spawn_blocking(|| {
        io_loop(game, player_name_clone, sender, exit_recv);
    });

    let (result, ()) = tokio::try_join!(event_loop, io_loop).unwrap();

    result
}

/// Listens for server broadcasts and prints their output.
async fn event_loop<E>(
    game: Arc<Mutex<ClientGame>>,
    player_handshake: Handshake,
    interface: &mut Interface<E>
) -> Result<(), E> {
    while let Some(msg) = interface.recv().await {
        match msg? {
            ServerMessage::Chat {
                player_name,
                message
            } => {
                println!("CHAT: <{}> {}", player_name, message);
            }
            ServerMessage::GameOver {reason, results } => {
                println!("Game Over ({reason})!\nBelow are the results:");
                for result in results.into_iter() {
                    println!("  {}", result);
                }
            }
            ServerMessage::DeadTile { player_name: player, dead_tile } => {
                println!("{player} traded in dead tile {dead_tile}.");
            }
            ServerMessage::GameStart {
                info,
                initial_hand,
            } => {
                let new_game = Game::start(&info);

                println!("The game has begun! Each player starts with ${}", info.starting_cash);
                println!("The order of play is {}", info.play_order.join(", "));
                println!("The board begins with {} on the board",
                // TODO: optimize
                info.tiles_placed.iter().map(Tile::to_string).collect::<Vec<_>>().join(", "));

                if let Some(initial_hand) = initial_hand {
                    println!("Your starting hand is: {}.",
                        initial_hand
                    );
                }

                game.lock().unwrap().start(
                    new_game.into(),
                    initial_hand.map(|h| h.into()),
                )
            },
            ServerMessage::CompanyDefunct { defunct, results } => {
                println!("Company {} has gone defunct!", defunct);
                for result in results.into_vec() {
                    println!("  {}", result);
                }
            },
            ServerMessage::Join { handshake } => {
                print!("{} joined", handshake.player_name);
                if handshake.spectating {
                    println!(" as a spectator.")
                } else {
                    println!(".");
                }
            }
            ServerMessage::Quit { handshake } => {
                println!("{} disconnected.", handshake.player_name)
            },
            ServerMessage::Shutdown => {
                println!("Server is shutting down. Press Enter to exit.");
                break;
            },
            ServerMessage::PlayerMove { action } => {

                // Decide how to update the game board
                game.lock().unwrap().update(&action);

                println!("{}", action);
            },
            ServerMessage::YourTurn { request: action } => {
                print!("Your turn to ");
                match action {
                    ActionRequest::PlayTile => {
                        println!("place a tile!")
                    },
                    ActionRequest::BuyStock => {
                        println!("buy stock!")
                    },
                    ActionRequest::ResolveMergeStock {
                        defunct,
                        into
                    } => println!("resolve the merge of {} into {}!", defunct, into),
                }
            },
            ServerMessage::TileDraw { tile } => {
                println!("You drew tile {tile}.");
            }
            ServerMessage::Invalid { reason } => {
                println!("Invalid message sent: {}", reason);
            },
        };
    }
    Ok(())
}

/// Blocking loop that sends commands to the servers. Continues to run until the
/// command sender encounters a SendError.
fn io_loop(
    game: Arc<Mutex<ClientGame>>,
    player_name: Box<str>,
    command_sender: mpsc::Sender<ClientMessage>,
    mut exit_notifier: oneshot::Receiver<()>,
) {
    let stdin = std::io::stdin();
    let mut buffer: String = String::new();

    loop {
        stdin.read_line(&mut buffer).unwrap();

        if let Ok(()) = exit_notifier.try_recv() {
            break;
        }

        let kind = parse_text(
            std::mem::replace(&mut buffer, String::new()),
            &player_name,
            &*game.lock().unwrap(),
        );
        let kind = match kind {
            Ok(v) => v,
            Err(err) => {
                if !matches!(err, CommandParseErr::EmptyInput) {
                    println!("Invalid command: {err}");
                }
                continue;
            },
        };
        if let Some(kind) = kind {
            let result = command_sender.blocking_send(kind);
            match result {
                Ok(()) => {},
                Err(_) => break,
            }
        }
    }
}

fn parse_text(
    line: String,
    player_name: &str,
    game: &ClientGame
) -> Result<Option<ClientMessage>, CommandParseErr> {
    enum CharDelim {
        Chat,
        PlayerAction,
        AdminCommand,
        GameDisplay,
    }
    use CharDelim::*;

    let command_delim = match line.chars().next() {
        Some('>') => Chat,
        Some('/') => PlayerAction,
        Some('#') => AdminCommand,
        Some('?') => GameDisplay,
        Some('\n') => return Err(CommandParseErr::EmptyInput),
        Some(_) => {
            println!("Invalid prefix character (use '>', '/' or '#').");
            return Err(CommandParseErr::EmptyInput);
        },
        // Inputs from the console always terminate with a newline.
        None => unreachable!(),
    };

    // Strip deliminator character and newline
    let line = &line[1..(line.len()-1)];

    Ok(match command_delim {
        Chat => Some(ClientMessage::Chat { message: line.to_owned().into_boxed_str() }),
        PlayerAction => {
            let action = parse_game_command(line)?;
            Some(ClientMessage::TakingTurn(action))
        },
        AdminCommand => {
            let command = parse_admin_command(line)?;
            Some(ClientMessage::Admin(command))
        },
        GameDisplay => {
            if let Some(game_obj) = game.game() {
                match &*line {
                    "board" => println!("{:?}", game_obj.board()),
                    "tiles" => {
                        if let Some(tiles) = game.hand() {
                            println!("{:?}", tiles)
                        }
                    }
                    _ => {}
                }
            } else {
                println!("no game in progress");
            }
            None
        }
    })
}
