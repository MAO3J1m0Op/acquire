use std::io;

use crate::game::kernel::Game;
use crate::game::tile::{FullHand, Tile};
use crate::game::{messages::*, Company, CompanyMap};
use crate::server::{ConnectionManager, Handshake, NewConnection};

use self::chat_panel::ChatPanel;
use self::command_buffer::CommandBuffer;
use self::terminal::{TermPanelCache, OverflowMode, TermWriteError};

/// The action panel is the main interface where the player decides what actions to take on their turn.
mod action_panel;
use action_panel::ActionPanel;
/// The board panel prints the game board, or the lobby information when there is not a game in progress.
mod board_panel;
use board_panel::BoardLobbyPanel;
/// The chat panel is responsible for printing chat and in-game messages.
mod chat_panel;
/// The command buffer manages the user typing and sending commands.
mod command_buffer;
pub mod terminal;
mod panels;

use panels::PanelTooSmallError;
use terminal::{setup_terminal, TermPanelUpdate};
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

    let (term, mut keys) = setup_terminal()?;

    dbg!("terminal made");

    std::thread::sleep(std::time::Duration::from_secs(1));

    // Print the panels
    //print_panels(&mut term, (100, 32)).unwrap();

    // Create the game
    let game = ClientGame::new(
        connection.handshake,
        connection.server_state.game_history
    );
    let mut panels = ClientPanels::new((&term).into(), game, connection.server_state.connections)
        // TODO: handle PanelTooSmallError
        .unwrap();

    loop {
        let msg: Option<ClientMessage> = tokio::select! {
            key = keys.recv() => {
                let key = match key {
                    Some(v) => v,
                    None => break,
                };

                match panels.process_key(key)? {
                    ClientPanelKeyProcessResult::Exit => break,
                    ClientPanelKeyProcessResult::Continue => None,
                    ClientPanelKeyProcessResult::SendMessage(client_message) => Some(client_message),
                }
            },
            msg = connection.interface.recv() => {
                // If receiver is closed, break
                let msg = match msg {
                    Some(v) => v,
                    None => break,
                };
                let msg = match msg {
                    Ok(v) => v,
                    // Exit client; report error from interface
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

struct ClientPanels {
    game: ClientGame,
    connections: ConnectionManager,
    command_buf: CommandBuffer,
    action_panel: ActionPanel,
    board_panel: BoardLobbyPanel,
    chat_panel: ChatPanel,
    keystroke_demander: KeystrokeDemander,
}

enum KeystrokeDemander {
    ActionPanel,
    ChatPanel,
    Exiting,
}

enum ClientPanelKeyProcessResult {
    /// The player wishes to exit
    Exit,
    /// Continue running; nothing special happens
    Continue,
    /// Server should continue running and send a [`ClientMessage`].
    SendMessage(ClientMessage),
}

impl ClientPanelKeyProcessResult {
    fn maybe_emit(value: Option<ClientMessage>) -> Self {
        match value {
            Some(msg) => Self::SendMessage(msg),
            None => Self::Continue,
        }
    }
}

impl ClientPanels {

    pub fn new(
        panel: TermPanelUpdate,
        game: ClientGame,
        connection_manager: ConnectionManager,
    ) -> Result<Self, PanelTooSmallError> {

        let split = PanelSplit::new(panel)?;

        Ok(Self {
            keystroke_demander: KeystrokeDemander::ActionPanel,
            game,
            connections: connection_manager,
            command_buf: CommandBuffer::new(split.command_buf),
            action_panel: ActionPanel::new(split.action_panel)?,
            board_panel: BoardLobbyPanel::new(split.board_panel),
            chat_panel: ChatPanel::new(split.chat_panel),
        })
    }

    /// Writes an error message onto the client.
    pub fn write_error(&mut self, error: &str)
        -> Result<(), TermWriteError>
    {
        self.keystroke_demander = KeystrokeDemander::ActionPanel;
        self.command_buf.write_error(error)
    }

    fn process_key(&mut self, key: Key) -> io::Result<ClientPanelKeyProcessResult> {

        use ClientPanelKeyProcessResult::*;

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

                        // Get the board. If no game is in progress, the action
                        // panel is assumed to be empty.
                        let board = self.game.game().map(|g| g.board());

                        if let Some(board) = board {
                            let msg = match self.action_panel.process_key(key, board) {
                                Ok(Some(msg)) => Some(msg),
                                Err(why) => {
                                    self.write_error(&why).unwrap();
                                    None
                                }
                                Ok(None) => None,
                            };
                            return Ok(ClientPanelKeyProcessResult::maybe_emit(msg));
                        }
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

                return Ok(ClientPanelKeyProcessResult::maybe_emit(msg));
            },
            KeystrokeDemander::Exiting => {
                if key == Key::Char('y') {
                    return Ok(Exit);
                } else {
                    // Stop trying to exit
                    self.write_error("").unwrap();
                    self.keystroke_demander = KeystrokeDemander::ActionPanel;
                }
            }
        }

        Ok(Continue)
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
                self.process_chat_message(player_name, message);
            },
            ServerMessage::Join { handshake } => {
                self.process_join_message(handshake);
            },
            ServerMessage::Quit { handshake } => {
                self.process_quit_message(handshake);
            },
            ServerMessage::PlayerMove { action } => {
                self.process_player_move_message(action);
            },
            ServerMessage::DeadTile { player_name, dead_tile } => {
                self.process_dead_tile_message(player_name, dead_tile);
            }
            ServerMessage::GameStart { info, initial_hand } => {
                self.process_game_start_message(info, initial_hand);
            },
            ServerMessage::CompanyDefunct { defunct, results } => {
                self.process_company_defunct_message(defunct, results);
            },
            ServerMessage::GameOver { reason, results } => {
                self.process_game_over_message(reason, results);
            },
            ServerMessage::Shutdown => return Ok(None),
            ServerMessage::YourTurn { request } => {
                if let Some(msg) = self.process_your_turn_message(request) {
                    return Ok(Some(Some(msg)))
                }
            },
            ServerMessage::TileDraw { tile } => {
                self.process_tile_draw_message(tile);
            }
            ServerMessage::Invalid { reason } => {
                self.write_error(&format!("Invalid message: {reason}")).unwrap();
            },
        }

        Ok(Some(None))
    }

    fn process_chat_message(&mut self, player_name: Box<str>, message: Box<str>) {
        let chat = format!("<{player_name}> {message}");
        self.chat_panel.add_message(chat.into_boxed_str());
    }

    fn process_join_message(&mut self, handshake: Handshake) {

        // Broadcast the message
        let spectate_msg = if handshake.spectating { " as a spectator" } else { "" };
        let chat = format!("JOIN: {} joined the game{spectate_msg}.", handshake.player_name);
        self.chat_panel.add_message(chat.into_boxed_str());

        // Connect the player
        self.connections.connect(handshake).unwrap();
    }


    fn process_quit_message(&mut self, handshake: Handshake) {

        // Disconnect the player
        assert!(self.connections.disconnect(&handshake.player_name));

        let chat = format!("JOIN: {} left the game.", &handshake.player_name);
        self.chat_panel.add_message(chat.into_boxed_str());
    }

    fn process_player_move_message(&mut self, action: TaggedPlayerAction) {
        self.chat_panel.add_message(
            action.to_string().into_boxed_str()
        );
        self.game.update(&action);

        // EDGE CASE: if the action panel is trying to produce an action
        // but the player uses the command buffer to send the action
        // instead, the action panel will become outdated. To fix this,
        // we clear the action panel upon receipt of a player action.
        self.action_panel.cancel_action();
    }

    fn process_dead_tile_message(&mut self, player_name: Box<str>, tile: Tile) {
        let msg = format!("{player_name} traded in dead tile {tile}.");
        self.chat_panel.add_message(msg.into_boxed_str());
    }

    fn process_game_start_message(&mut self, start_info: GameStart, initial_hand: Option<FullHand>) {

        // Start a new game
        let game = Game::start(&start_info).into();
        self.board_panel.draw_board(&game);
        let hand = initial_hand.map(|hand| hand.into());
        self.game.start(game, hand);

        // Send corresponding chat messages
        let msg = "Game started!".to_owned().into_boxed_str();
        self.chat_panel.add_message(msg);
        if let Some(initial_hand) = initial_hand {
            let msg = format!("Your starting hand is: {}.", initial_hand).into_boxed_str();
            self.chat_panel.add_message(msg);
        }
    }

    fn process_company_defunct_message(&mut self, defunct: Company, results: Box<[PrincipleShareholderResult]>) {
        let msg = format!(
            "Company {defunct} has gone defunct! Here are the results:"
        ).into_boxed_str();
        self.chat_panel.add_message(msg);
        for result in results.iter() {
            let msg = result.to_string().into_boxed_str();
            self.chat_panel.add_message(msg);
        }
    }

    fn process_game_over_message(&mut self, reason: GameOver, results: Box<[FinalResult]>) {

        // End the game and draw the lobby
        self.game.end();
        self.board_panel.draw_lobby(&self.connections);

        let msg = format!("Game Over! {reason}. Here are the results:").into_boxed_str();
        self.chat_panel.add_message(msg);
        results.into_iter().for_each(|result| {
            let msg = format!("  [{}] {} with ${}",
                result.place, result.player_name, result.final_money);
            self.chat_panel.add_message(msg.into_boxed_str())
        });
    }

    /// May return a message immediately if short-circuit logic takes place
    /// (i.e. a forced move such as a buying stock turn with no money).
    fn process_your_turn_message(&mut self, request: ActionRequest) -> Option<ClientMessage> {

        // Processes the request
        match request {
            ActionRequest::PlayTile => self.action_panel.request_place_tile(),
            ActionRequest::BuyStock => {

                // Figures out which companies are available
                todo!();

                self.action_panel.request_buy_stock(todo!());
            },
            ActionRequest::ResolveMergeStock { defunct, into } => {
                self.action_panel.request_resolve_merge_stock(());
            },
        }
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

            // If we're receiving a buy-stock request, we're assuming
            // the game is already started.
            let game = self.game.game().unwrap();

            // SHORT CIRCUIT: if there's no stock to buy, skip buying stock
            let none_exist = CompanyMap::new(&()).map(|cmp, _| game.board().company_exists(cmp))
                .iter().all(|(_, exists)| !exists);
            if none_exist {
                return Some(ClientMessage::TakingTurn(
                    PlayerAction::BuyStock { stock: [None; 3] }))
            }

            // SHORT CIRCUIT: if the player can't afford stock, then skip
            // buying stock
            let player_name = &self.game.client.player_name;
            let player_money = game.players().get(player_name).unwrap().money;
            let cant_afford = CompanyMap::new(&())
                .map(|cmp, _| game.board().stock_price(cmp) > player_money)
                .iter().all(|(_, &too_expensive)| too_expensive);

            if cant_afford {
                let msg = "You can't afford any stock!".to_owned().into_boxed_str();
                self.chat_panel.add_message(msg);
                return Some(ClientMessage::TakingTurn(
                    PlayerAction::BuyStock { stock: [None; 3] }))
            }
        }
        None
    }

    fn process_tile_draw_message(&mut self, tile: Tile) {

        // Assumes a game is in progress
        let game = self.game.game().unwrap();

        // Assumes the player's hand isn't already full
        self.action_panel.provide_tile(tile, game.board()).unwrap();

        let msg = format!("You drew tile {}.", tile).into_boxed_str();
        self.chat_panel.add_message(msg);
    }


    /// Call this function any time the size of the terminal changes. This
    /// resizes each sub-panel and re-renders everything.
    fn resize(&mut self, new_panel: TermPanelUpdate) -> Result<(), PanelTooSmallError> {

        let split = PanelSplit::new(new_panel)?;

        self.action_panel.resize(split.action_panel)?;
        self.board_panel = BoardLobbyPanel::new(split.board_panel);
        self.chat_panel.resize(split.chat_panel);
        self.command_buf.resize(split.command_buf);

        Ok(())
    }
}

struct PanelSplit<'c> {
    pub action_panel: TermPanelUpdate<'c>,
    pub board_panel: TermPanelUpdate<'c>,
    pub command_buf: TermPanelUpdate<'c>,
    pub chat_panel: TermPanelUpdate<'c>,
    _private_constructor: (),
}

impl<'c> PanelSplit<'c> {
    /// Creates a new panel split and prints the ASCII border around the panels
    pub fn new(mut panel: TermPanelUpdate<'c>) -> Result<Self, PanelTooSmallError> {

        // Create the panels for the borders
        let (top_border, bottom_border) = panel.shave_vert(1, 1)?;
        let (left_border, right_border) = panel.shave_horiz(2, 2)?;

        // Split the panel in two, generate the middle padding
        let (mut left, right) = panel.split_horiz(0.5);
        let (_, middle_border) = left.shave_horiz(0, 1)?;

        // Split the right panel into chat and cmd
        let mut chat_panel = right;
        let (_, mut command_buf) = chat_panel.shave_vert(0, 2)?;
        let (chat_cmd_border, _) = command_buf.shave_vert(1, 0)?;

        // The left panel are the game panels. Decide which axis to split on
        let (board_panel, action_panel) = if left.dim().size.0 < left.dim().size.1 {
            left.split_horiz(0.5)
        } else {
            left.split_vert(0.5)
        };

        // Print into the border panels
        // Unwrap: all of these are valid ASCII characters
        let mut top_border = TermPanelCache::from(top_border);
        let mut bottom_border = TermPanelCache::from(bottom_border);
        let mut left_border = TermPanelCache::from(left_border);
        let mut middle_border = TermPanelCache::from(middle_border);
        let mut right_border = TermPanelCache::from(right_border);
        let mut chat_cmd_border = TermPanelCache::from(chat_cmd_border);
        top_border.fill('=').unwrap();
        bottom_border.fill('=').unwrap();
        left_border.fill('|').unwrap();
        middle_border.fill('|').unwrap();
        right_border.fill('|').unwrap();
        chat_cmd_border.write(OverflowMode::Truncate, |writer| {
            writer.write_str("- CHAT ").unwrap();
            while writer.can_write_char() { writer.write_char('-').unwrap(); }
        });

        Ok(Self {
            action_panel,
            board_panel,
            command_buf,
            chat_panel,
            _private_constructor: (),
        })
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
