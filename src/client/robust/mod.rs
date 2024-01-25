use std::io;

use crate::game::{messages::*, CompanyMap};
use crate::server::{ConnectionManager, NewConnection};

use self::game_panels::GamePanels;
use self::chat_panel::ChatPanel;
use self::command_buffer::CommandBuffer;
use self::terminal::{TermPanel, OverflowMode, TermWriteError};

/// The chat panel is responsible for printing chat and in-game messages.
mod chat_panel;
/// The command buffer manages the user typing and sending commands.
mod command_buffer;
/// This shows both the board and the menu the player uses to input moves.
mod game_panels;
pub mod terminal;
mod panels;

use termion::event::Key;

use super::{CommandParseErr, parse_game_command, parse_admin_command, ClientGame};

/// Starts the client for a [`FallibleInterface`] that throws I/O errors.
#[inline]
pub async fn run_io(connection: NewConnection<io::Error>) -> io::Result<()> {
    match run(connection).await {
        Ok(result) => result,
        Err(err) => Err(err),
    }
}
    
/// Starts the client for the specified player interface.
pub async fn run<E>(mut connection: NewConnection<E>) -> io::Result<Result<(), E>> {

    dbg!(&connection.server_state);

    dbg!("terminal making");

    let (term, mut keys) = TermPanel::new()?;

    dbg!("terminal made");

    std::thread::sleep(std::time::Duration::from_secs(1));

    // Print the panels
    //print_panels(&mut term, (100, 32)).unwrap();

    // Create the game
    let game = ClientGame::new(
        connection.handshake,
        connection.server_state.game_history
    );
    let connections = &mut connection.server_state.connections;

    let mut panels = ClientPanels::new(term, game, connections)?;

    panels.rerender_panels();

    loop {
        let msg = tokio::select! {
            key = keys.recv() => {
                let key = match key {
                    Some(v) => v,
                    None => break,
                };

                match panels.process_key(key)? {
                    Some(option) => option,
                    None => break,
                }
            },
            msg = connection.interface.recv() => {
                let msg = match msg {
                    Some(v) => v,
                    None => break,
                };
                let msg = match msg {
                    Ok(v) => v,
                    Err(e) => return Ok(Err(e)),
                };

                match panels.process_msg(msg)? {
                    Some(option) => option,
                    None => break,
                }
            }
        };

        if let Some(msg) = msg {
            let result = connection.interface.sender().send(msg).await;
            if result.is_err() { break; }
        }
    }

    Ok(connection.interface.close().await)
}

struct ClientPanels<'c> {
    command_buf: CommandBuffer,
    game_panel: GamePanels<'c>,
    chat_panel: ChatPanel,
    keystroke_demander: KeystrokeDemander,
}

enum KeystrokeDemander {
    ActionPanel,
    ChatPanel,
    Exiting,
}

impl<'c> ClientPanels<'c> {

    pub fn new(
        panel: TermPanel,
        game: ClientGame,
        connection_manager: &'c mut ConnectionManager,
    ) -> io::Result<Self> {
        // Create the panels with zero size
        let mut me = Self {
            command_buf: CommandBuffer::new(),
            game_panel: GamePanels::new(
                game,
                connection_manager
            ),
            chat_panel: ChatPanel::new(),
            keystroke_demander: KeystrokeDemander::ActionPanel,
        };

        // ...then size and render accordingly
        me.resize(panel)?;

        Ok(me)
    }

    /// Writes an error message onto the client.
    pub fn write_error(&mut self, error: &str)
        -> Result<(), TermWriteError>
    {
        self.keystroke_demander = KeystrokeDemander::ActionPanel;
        self.command_buf.write_error(error)
    }

    /// Returns a client message that may have been produced.
    /// # Return value
    /// 
    /// * `Err(...)` indicates an I/O error.
    /// * `Ok(None)` indicates that the player wishes to exit.
    /// * `Ok(Some(None))` indicates that the server should continue, but no
    ///   client message needs to be sent.
    /// * `Ok(Some(Some(...)))` indicates a client message should be sent.
    fn process_key(&mut self, key: Key) -> io::Result<Option<Option<ClientMessage>>> {

        // Decide which panel gets the key
        match &self.keystroke_demander {
            KeystrokeDemander::ActionPanel => {
                match key {
                    // Symbols that globally enter the command buffer
                    Key::Char('>') | Key::Char('/') | Key::Char('#') => {
                        self.keystroke_demander = KeystrokeDemander::ChatPanel;
                        let none = self.command_buf.process_key(key);
                        assert!(none.is_none());
                    },
                    // Ask to confirm the exit request
                    Key::Esc => {
                        self.write_error("Type 'y' to confirm exit").unwrap();
                        self.keystroke_demander = KeystrokeDemander::Exiting;
                    }
                    _ => {
                        let msg = match self.game_panel.process_key(key) {
                            Some(Ok(msg)) => Some(msg),
                            Some(Err(why)) => {
                                self.write_error(&why).unwrap();
                                None
                            }
                            None => None,
                        };
                        let msg = msg.map(|m| ClientMessage::TakingTurn(m));
                        
                        return Ok(Some(msg));
                    }
                }
            },
            KeystrokeDemander::ChatPanel => {

                let option = self.command_buf.process_key(key);

                // Handle the command, or write an error if the command failed
                let msg = option.map(|(command, mode)| {
                    match parse_command(mode, command.into_boxed_str()) {
                        Ok(cmd) => Some(cmd),
                        Err(e) => {
                            self.write_error(&e.to_string()).unwrap();
                            None
                        }
                    }
                }).flatten();

                // If a command was produced, then focus should be shifted away
                // from the buffer.
                if msg.is_some() || key == Key::Esc {
                    self.keystroke_demander = KeystrokeDemander::ActionPanel;
                }

                return Ok(Some(msg));
            },
            KeystrokeDemander::Exiting => {
                if key == Key::Char('y') {
                    // Exit
                    return Ok(None);
                } else {
                    // Stop trying to exit
                    self.write_error("").unwrap();
                    self.keystroke_demander = KeystrokeDemander::ActionPanel;
                }
            }
        }

        Ok(Some(None))
    }

    /// # Return value
    /// 
    /// * `Err(...)` indicates an I/O error.
    /// * `Ok(None)` indicates that a [`ServerBroadcast::Shutdown`] was received
    ///   and the server is closing.
    /// * `Ok(Some(None))` indicates that the server should continue, but no
    ///   client message needs to be sent.
    /// * `Ok(Some(Some(...)))` indicates a client message should be sent.
    fn process_msg(&mut self, msg: ServerMessage)
        -> io::Result<Option<Option<ClientMessage>>>
    {
        match dbg!(msg) {
            ServerMessage::Chat { player_name, message } => {
                let chat = format!("<{player_name}> {message}");
                self.chat_panel.add_message(chat.into_boxed_str());
            },
            ServerMessage::Join { handshake } => {

                // Broadcast the message
                let spectate_msg = if handshake.spectating { " as a spectator" } else { "" };
                let chat = format!("JOIN: {} joined the game{spectate_msg}.", handshake.player_name);
                self.chat_panel.add_message(chat.into_boxed_str());

                // Connect the player
                self.game_panel.connections_mut(
                    |connections| connections.connect(handshake).unwrap()
                );
            },
            ServerMessage::Quit { handshake } => {

                // Disconnect the player
                self.game_panel.connections_mut(
                    |connections| assert!(connections.disconnect(&handshake.player_name))
                );

                let chat = format!("JOIN: {} left the game.", &handshake.player_name);
                self.chat_panel.add_message(chat.into_boxed_str());
            },
            ServerMessage::PlayerMove { action } => {
                self.chat_panel.add_message(
                    action.to_string().into_boxed_str()
                );
                self.game_panel.update_game(&action);

                // EDGE CASE: if the action panel is trying to produce an action
                // but the player uses the command buffer to send the action
                // instead, the action panel will become outdated. To fix this,
                // we clear the action panel upon receipt of a player action.
                self.game_panel.cancel_action();
            },
            ServerMessage::DeadTile { player_name: player, dead_tile } => {
                let msg = format!("{player} traded in dead tile {dead_tile}.");
                self.chat_panel.add_message(msg.into_boxed_str());
            }
            ServerMessage::GameStart { info, initial_hand } => {

                self.game_panel.start_game(&info, initial_hand);

                let msg = "Game started!".to_owned().into_boxed_str();
                self.chat_panel.add_message(msg);
                if let Some(initial_hand) = initial_hand {
                    let msg = format!("Your starting hand is: {}.", initial_hand).into_boxed_str();
                    self.chat_panel.add_message(msg);
                }
            },
            ServerMessage::CompanyDefunct { defunct, results } => {
                let msg = format!(
                    "Company {defunct} has gone defunct! Here are the results:"
                ).into_boxed_str();
                self.chat_panel.add_message(msg);
                for result in results.iter() {
                    let msg = result.to_string().into_boxed_str();
                    self.chat_panel.add_message(msg);
                }
            },
            ServerMessage::GameOver { reason, results } => {

                self.game_panel.end_game();

                let msg = format!("Game Over! {reason}. Here are the results:").into_boxed_str();
                self.chat_panel.add_message(msg);
                results.into_iter().for_each(|result| {
                    let msg = format!("  [{}] {} with ${}",
                        result.place, result.player_name, result.final_money);
                    self.chat_panel.add_message(msg.into_boxed_str())
                });
            },
            ServerMessage::Shutdown => return Ok(None),
            ServerMessage::YourTurn { request } => {
                self.game_panel.request_action(request);
                let request_msg = match request {
                    ActionRequest::PlayTile => "place a tile",
                    ActionRequest::BuyStock => "buy stock",
                    ActionRequest::ResolveMergeStock {
                        defunct: _, into: _
                    } => "resolve your stock",
                };
                let msg = format!("Your turn to {request_msg}!");
                self.chat_panel.add_message(msg.into_boxed_str());

                if matches!(request, ActionRequest::BuyStock) {
                    let game = self.game_panel.game().game().unwrap();
                    // SHORT CIRCUIT: if there's no stock to buy, skip buying stock
                    let none_exist = CompanyMap::new(&()).map(|cmp, _| game.board().company_exists(cmp))
                        .iter().all(|(_, exists)| !exists);
                    if none_exist {
                        return Ok(Some(Some(ClientMessage::TakingTurn(
                            PlayerAction::BuyStock { stock: [None; 3] })))
                        )
                    }

                    // SHORT CIRCUIT: if the player can't afford stock, then skip
                    // buying stock
                    let player_name = &self.game_panel.game().client.player_name;
                    let player_money = game.players().get(player_name).unwrap().money;
                    let cant_afford = CompanyMap::new(&())
                        .map(|cmp, _| game.board().stock_price(cmp) > player_money)
                        .iter().all(|(_, &too_expensive)| too_expensive);

                    if cant_afford {
                        let msg = "You can't afford any stock!".to_owned().into_boxed_str();
                        self.chat_panel.add_message(msg);
                        return Ok(Some(Some(ClientMessage::TakingTurn(
                            PlayerAction::BuyStock { stock: [None; 3] })))
                        )
                    }
                }
            },
            ServerMessage::TileDraw { tile } => {
                self.game_panel.draw_tile(tile);

                let msg = format!("You drew tile {}.", tile).into_boxed_str();
                self.chat_panel.add_message(msg);
            }
            ServerMessage::Invalid { reason } => {
                self.write_error(&format!("Invalid message: {reason}")).unwrap();
            },
        }

        Ok(Some(None))
    }

    /// Call this function any time the size of the terminal changes. This
    /// resizes each sub-panel and re-renders everything.
    fn resize(&mut self, mut new_panel: TermPanel) -> io::Result<()> {
        // Create the panels for the borders
        let (mut top_border, mut bottom_border) = new_panel.shave_vert(1, 1).unwrap();
        let (mut left_border, mut right_border) = new_panel.shave_horiz(2, 2).unwrap();

        // Split the panel in two, generate the middle padding
        let (mut left, right) = new_panel.split_horiz(0.5);
        let (_, mut middle_border) = left.shave_horiz(0, 1).unwrap();

        // Split the right panel into chat and cmd
        let mut chat = right;
        let (_, mut cmd) = chat.shave_vert(0, 2).unwrap();
        let (mut chat_cmd_border, _) = cmd.shave_vert(1, 0).unwrap();

        // Print into the border panels
        top_border.fill('=').unwrap();
        bottom_border.fill('=').unwrap();
        left_border.fill('|').unwrap();
        middle_border.fill('|').unwrap();
        right_border.fill('|').unwrap();
        chat_cmd_border.write(OverflowMode::Truncate, |writer| {
            writer.write_str("- CHAT ").unwrap();
            while writer.can_write_char() { writer.write_char('-').unwrap(); }
        });

        self.chat_panel.resize(chat);
        self.game_panel.resize(left);
        self.command_buf.resize(cmd);

        Ok(())
    }

    pub fn rerender_panels(&mut self) {
        self.game_panel.render();
        self.chat_panel.render();
        self.command_buf.render();
    }
}

/// Parses a command produced by the command buffer. Sends the mode in which the
/// buffer was produced.
fn parse_command(buffer_mode: command_buffer::BufferMode, command: Box<str>)
    -> Result<ClientMessage, CommandParseErr>
{
    match buffer_mode {
        command_buffer::BufferMode::Chat => {
            Ok(ClientMessage::Chat {
                message: command
            })
        },
        command_buffer::BufferMode::Command => {
            Ok(ClientMessage::TakingTurn(
                parse_game_command(&command)?
            ))
        },
        command_buffer::BufferMode::Admin => {
            Ok(ClientMessage::Admin(
                parse_admin_command(&command)?
            ))
        },
    }
}
