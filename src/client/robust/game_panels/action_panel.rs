use crate::client::robust::terminal::{OverflowMode, NiceFgColor, TermWriter};
use crate::client::robust::terminal::TermPanel;
use crate::game::{messages::*, Company, CompanyMap};
use crate::game::tile::{Hand, Tile};

/// Stores the state and renders whatever menu is in progress.
#[derive(Debug)]
pub struct ActionPanel {
    action: Option<ActionState>,
    tile_layout: TileLayout,
    panel: Option<TermPanel>,
}

impl ActionPanel {

    /// Creates a new [`ActionPanel`] of size zero. It must be resized later.
    pub fn new() -> Self {
        Self {
            panel: None,
            action: None,
            tile_layout: TileLayout::Column,
        }
    }

    /// Informs the action panel that the player must make an action.
    pub fn request_action(
        &mut self, request: ActionPanelRequest, hand: Option<&Hand>,
    ) {
        self.action = Some(match request {
            ActionPanelRequest::PlaceTile => {
                ActionState::ChoosingTile(0)
            },
            ActionPanelRequest::BuyStock { available_companies } => {
                let available_companies: Vec<_> = available_companies.iter()
                    .filter_map(|(company, &available)| {
                        if available { Some(company) } else { None }
                    })
                    .collect();
                ActionState::BuyingStock {
                    purchases: [None; 3],
                    index: available_companies.len() as u8,
                    available_companies,
                }
            },
            ActionPanelRequest::FoundCompany { tile_placed, available_companies } => {
                let available_companies = available_companies.iter()
                    .filter_map(|(company, &available)| {
                        if available { Some(company) } else { None }
                    })
                    .collect();
                ActionState::FoundingCompany {
                    tile_placed,
                    available_companies,
                    selected: 0,
                }
            },
            ActionPanelRequest::Merge { tile_placed, participants } => {
                let participants = participants.iter()
                    .filter_map(|(company, &available)| {
                        if available { Some(company) } else { None }
                    })
                    .collect();
                ActionState::Merging {
                    tile_placed,
                    participants,
                    selected: 0,
                }
            },
            ActionPanelRequest::ResolveMergeStock { count } => {
                ActionState::ResolvingMergeStock {
                    selling: 0,
                    keeping: count,
                    trading: 0,
                    highlighted: 0,
                }
            }
        });
        self.render(hand)
    }

    /// Cancels whatever action was in progress and clears the action panel.
    pub fn cancel_action(&mut self, hand: Option<&Hand>) {
        self.action = None;
        self.render(hand)
    }

    /// Processes a single key from the user. If that key completes the action,
    /// this function returns [`Some`] with the completed action.
    pub fn process_key(&mut self,
        key: termion::event::Key,
        hand: Option<&Hand>,
    ) -> Option<PlayerAction> {
        use termion::event::Key;

        match self.action.take() {
            Some(ActionState::BuyingStock { purchases, available_companies, index }) => {

                let action = match key {
                    // Cycle between the companies to buy stock
                    Key::Left => {
                        let index = (index as i8 - 1) % (available_companies.len() as i8 + 1);
                        self.action = Some(ActionState::BuyingStock {
                            purchases, available_companies, index: index as u8
                        });
                        None
                    },
                    Key::Right => {
                        let index = (index + 1) % (available_companies.len() as u8 + 1);
                        self.action = Some(ActionState::BuyingStock {
                            purchases, available_companies, index
                        });
                        None
                    },

                    // Enter advances to the next stock
                    Key::Char('\n') => {
                        if index == 2 {
                            let stock = purchases
                                .map(|p| p.map(|p| available_companies[p as usize]));
                            Some(PlayerAction::BuyStock { stock })
                        } else {
                            self.action = Some(ActionState::BuyingStock {
                                purchases,
                                available_companies,
                                index: index + 1,
                            });
                            None
                        }
                    },
                    // Invalid key does nothing
                    _ => {
                        self.action = Some(ActionState::BuyingStock {
                            purchases, available_companies, index
                        });
                        None
                    }
                };

                self.render(hand);

                action
            },
            Some(ActionState::FoundingCompany { tile_placed, available_companies, selected }) => {
                todo!();
            },
            Some(ActionState::Merging { tile_placed, participants, selected }) => {
                todo!();
            }
            Some(ActionState::ChoosingTile(tile_index)) => {
                let offset = match key {
                    Key::Left => Some(-1),
                    Key::Right => Some(1),
                    Key::Up => Some(-(self.tile_layout.vertical_count() as i8)),
                    Key::Down => Some(self.tile_layout.vertical_count() as i8),
                    Key::Char('\n') => None,
                    _ => return None,
                };

                match offset {
                    // Arrow key was pressed
                    Some(offset) => {
                        let new_offset = (tile_index as i8 + offset).rem_euclid(6);
                        self.action = Some(ActionState::ChoosingTile(new_offset as usize));

                        // Do a short-circuit re-rendering of the tiles
                        if let Some(ref mut panel) = self.panel {
                            panel.write(OverflowMode::Truncate, |writer| {
                                let hand = hand.unwrap(); // The hand should be [`Some`] if we're working with tiles.
                                write_tiles(writer, hand, Some(new_offset as usize), self.tile_layout);
                            })
                        }
                        
                        None
                    },
                    // Enter was pressed, so we emit the action
                    None => {
                        let action = Some(PlayerAction::PlayTile {
                            placement: TilePlacement {
                                tile: {
                                    // Game should be in progress if action
                                    // panel is taking actions.
                                    let hand = hand.unwrap();
                                    let mut iter = hand.iter();
                                    for _ in 0..tile_index { iter.next(); }
                                    *iter.next().unwrap()
                                },
                                implication: None
                            }
                        });
                        self.render(hand);
                        action
                    },
                }
            },
            Some(ActionState::ResolvingMergeStock { selling, keeping, trading, highlighted }) => {
                todo!()
            }
            None => None,
        }
    }

    /// Resizes and renders the panel.
    pub fn resize(&mut self, new_panel: TermPanel, hand: Option<&Hand>) {
        // [ ] X-XX  [ ] X-XX
        // [ ] X-XX  [ ] X-XX
        // [ ] X-XX  [ ] X-XX
        const MIN_WIDTH_3X2: u16 = 8*2 + 2;
        // [ ] X-XX  [ ] X-XX  [ ] X-XX
        // [ ] X-XX  [ ] X-XX  [ ] X-XX
        const MIN_WIDTH_2X3: u16 = 8*3 + 4;

        self.tile_layout = {
            if new_panel.dim().size.0 > MIN_WIDTH_2X3 { TileLayout::Grid2x3 }
            else if new_panel.dim().size.0 > MIN_WIDTH_3X2 { TileLayout::Grid3x2 }
            else { TileLayout::Column }
        };

        self.panel = Some(new_panel);

        self.render(hand);
    }

    pub fn render(&mut self, hand: Option<&Hand>) {
        if let Some(ref mut panel) = self.panel {
            use ActionState::*;

            panel.clear();
            panel.write(OverflowMode::Truncate, |writer| {
                if let Some(hand) = hand {

                    match &self.action {
                        None => {
                            write_tiles(writer, hand, None, self.tile_layout);
                        },
                        Some(ChoosingTile(tile_index)) => {
                            write_tiles(writer, hand, Some(*tile_index), self.tile_layout);
                        },
                        Some(BuyingStock { purchases: _, available_companies, index }) => {
                            write_tiles(writer, hand, None, self.tile_layout);
        
                            writer.set_overflow_mode(OverflowMode::Wrap);
        
                            writer.new_line();
                            writer.new_line();
                            writer.write_str("Choose which stock to buy").unwrap();
                            writer.new_line();
                            writer.new_line();
        
                            writer.set_overflow_mode(OverflowMode::Truncate);
        
                            company_chooser(
                                writer,
                                available_companies.get(*index as usize).map(|x| *x));
                        },
                        Some(FoundingCompany { tile_placed, available_companies, selected }) => {
                            write_tiles(writer, hand, None, self.tile_layout);
        
                            writer.write_str("Found which company?").unwrap();
                        },
                        Some(Merging { tile_placed: _, participants, selected }) => {
                            write_tiles(writer, hand, None, self.tile_layout);
        
                            writer.write_str("Found which company?").unwrap();
                        },
                        Some(ResolvingMergeStock { selling, keeping, trading, highlighted }) => {
                            
                        }
                    }
                }    
            });
        }
    }
}

fn write_tiles(
    writer: &mut TermWriter,
    hand: &Hand,
    tile_index: Option<usize>,
    tile_layout: TileLayout,
) {
    let mut hand_iter = hand.iter();

    match tile_layout {
        TileLayout::Grid2x3 => {
            'outer: for row in 0..2 {
                for col in 0..3 {
                    if let Some(tile) = hand_iter.next() {
                        let idx = row * 3 + col;
                        write_tile(writer, *tile, Some(idx) == tile_index);
                        writer.write_str("  ").unwrap();
                    } else { break 'outer }
                }
                writer.new_line();
            }
        },
        TileLayout::Grid3x2 => {
            'outer: for row in 0..3 {
                for col in 0..2 {
                    if let Some(tile) = hand_iter.next() {
                        let idx = row * 2 + col;
                        write_tile(writer, *tile, Some(idx) == tile_index);
                        writer.write_str("  ").unwrap();
                    } else { break 'outer }
                }
                writer.new_line();
            }
        },
        TileLayout::Column => {
            for row in 0..6 {
                if let Some(tile) = hand_iter.next() {
                    write_tile(writer, *tile, Some(row) == tile_index);
                    writer.new_line();
                } else { break }
            }
        },
    }

    // Print instructions
    if tile_index.is_some() {
        let mode = writer.overflow_mode();
        writer.set_overflow_mode(OverflowMode::Wrap);
        writer.new_line();
        writer.write_fg_colored(
            "Select a tile to play\n", termion::color::LightWhite
        ).unwrap();
        writer.set_overflow_mode(mode);
    }
}

fn write_tile(writer: &mut TermWriter, tile: Tile, selected: bool) {

    // Write the selection box
    writer.write_fg_colored('[', termion::color::Yellow).unwrap();
    if selected {
        writer.write_bg_colored(' ', termion::color::White)
    } else {
        writer.write_char(' ').map(|_| {})
    }.unwrap();
    writer.write_fg_colored(']', termion::color::Yellow).unwrap();

    writer.write_char(' ').unwrap();

    // Write the tile name
    writer.write(&&*tile.to_string()).unwrap();

    // Determine if an extra space needs to be typed
    if tile.row() < 10 {
        writer.write_char(' ').unwrap();
    }
}

fn company_chooser(writer: &mut TermWriter, company: Option<Company>) {

    // This is a bad design choice by Termion...
    use std::fmt;
    #[derive(Debug)]
    struct NewType(Option<Company>);
    impl termion::color::Color for NewType {
        fn write_fg(&self, f: &mut fmt::Formatter) -> fmt::Result {
            match self.0 {
                Some(c) => c.write_fg(f),
                None => termion::color::Reset.write_fg(f),
            }
        }

        fn write_bg(&self, f: &mut fmt::Formatter) -> fmt::Result {
            match self.0 {
                Some(c) => c.write_bg(f),
                None => termion::color::Reset.write_bg(f),
            }
        }
    }
    impl NiceFgColor for NewType {
        fn write_nice_fg(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self.0 {
                Some(c) => c.write_nice_fg(f),
                None => termion::color::Reset.write_nice_fg(f),
            }
        }
    }

    writer.write_str("    < ").unwrap();
    let string = company.map(|c| c.to_string()).unwrap_or("---".to_owned());
    writer.write_bg_colored(&*string, NewType(company)).unwrap();
    writer.write_str(" >").unwrap();
}

/// An action request sent directly to the panel. This allows the
/// [`ActionPanel`] to be unknowing of the current state of the game. 
pub enum ActionPanelRequest {
    PlaceTile,
    BuyStock { available_companies: CompanyMap<bool> },
    FoundCompany {
        tile_placed: Tile,
        /// Map with value `true` for all companies that are off the board and
        /// available to be founded. It is an invalid state to construct this
        /// variant with a map containing all `false` values.
        available_companies: CompanyMap<bool>,
    },
    Merge {
        tile_placed: Tile,
        /// Map with value `true` for all companies that are off the board and
        /// available to be founded. It is an invalid state to construct this
        /// variant with a map containing all `false` values.
        participants: CompanyMap<bool>,
    },
    ResolveMergeStock {
        count: u8,
    }
}

/// Used to track the internal state of the action panel.
#[derive(Debug)]
enum ActionState {
    ChoosingTile(usize),
    BuyingStock {
        purchases: [Option<u8>; 3],
        available_companies: Vec<Company>,
        index: u8,
    },
    FoundingCompany {
        tile_placed: Tile,
        available_companies: Vec<Company>,
        selected: u8,
    },
    Merging {
        tile_placed: Tile,
        participants: Vec<Company>,
        selected: u8,
    },
    ResolvingMergeStock {
        selling: u8,
        keeping: u8,
        trading: u8,
        highlighted: u8,
    }
}

#[derive(Debug, Clone, Copy)]
enum TileLayout {
    /// Preferred option; 2 rows, 3 columns. 
    Grid2x3,
    /// 3 rows, 2 columns.
    Grid3x2,
    /// 6 rows, 1 column.
    Column,
}

impl TileLayout {
    /// When the down arrow or up arrow are pressed, this is the number of
    /// indices to advance.
    pub fn vertical_count(&self) -> u8 {
        match self {
            TileLayout::Grid2x3 => 3,
            TileLayout::Grid3x2 => 2,
            TileLayout::Column => 1,
        }
    }
}

// use termion::event::Key;
// use tokio::sync::mpsc;

// use crate::game::board::{Tile, Board};
// use crate::game::messages::*;

// use super::panels::PanelDim;

// pub(super) struct ActionPanel {

// }

// impl ActionPanel {

// }

// impl ActionPanelGame {

//     /// Directs the action panel to prompt a specific action from the user.
//     pub fn request_action(&self, request: ActionRequest) {
//         todo!();
//     }

//     /// Informs the action panel of a tile being placed on the board.
//     pub fn place_tile(&mut self, placement: TilePlacement) {
//         self.board.place_tile(placement);

//         // TODO: rendering
//     }

//     /// Informs the action panel of a merge resolution.
//     /// 
//     /// # Panics
//     /// 
//     /// When called, this function assumes that there is already a merge in
//     /// play. If that is not the case, this function panics.
//     pub fn resolve_merge(&mut self, merge: &Merge) {
//         self.board.resolve_merge(merge);

//         // TODO: rendering
//     }

//     /// Informs the action panel of an update to the tiles in the player's hand.
//     pub async fn refresh_tiles(&mut self, tiles: [Tile; 6]) {
//         self.tiles = tiles;
//         // TODO: Update the tiles on-screen
//     }
// }

// // use termion::event::Key;
// // use tokio::sync::mpsc;

// // use crate::game::{Tile, Board, PlayerData};
// // use crate::game::messages::{GameOver, ActionRequest, TilePlacementImplication};

// // use super::{PanelDim, Panel, PanelHandler};

// // pub(super) struct ActionPanel {
// //     pub player_name: String
// // }

// // /// Sends data to the action panel.
// // #[derive(Debug, Clone)]
// // pub(super) struct ActionPanelHandler {
// //     input: mpsc::Sender<Input>,
// //     tiles: mpsc::Sender<Tile>,
// // }

// // impl Panel for ActionPanel {

// //     type Handler = ActionPanelHandler;

// //     /// Starts the action panel in a separate process, returning handles to it.
// //     fn start(self,
// //         key_receiver: mpsc::Receiver<Key>,
// //         rendering_sender: mpsc::Sender<String>,
// //         dim: PanelDim,
// //     ) -> Self::Handler {
// //         // Make the channels
// //         let (input_sender, input_receiver) = mpsc::channel(32);
// //         let (tile_sender, tile_receiver) = mpsc::channel(8);

// //         let mut panel = RunningActionPanel {
// //             dim,
// //             rendering_sender,
// //             input: input_receiver,
// //             player_name: self.player_name,
// //             tiles: tile_receiver,
// //         };

// //         tokio::spawn(async move {
// //             panel.clear().await;

// //             // Action panel event loop
// //             loop {
// //                 let (board, data) = match panel.lobby().await {
// //                     Some(x) => x,
// //                     None => break,
// //                 };
// //                 match panel.game(board, data).await {
// //                     Some(x) => x,
// //                     None => break,
// //                 }
// //             }
// //         });

// //         ActionPanelHandler {
// //             input: input_sender,
// //             tiles: tile_sender,
// //         }
// //     }
// // }

// // impl ActionPanelHandler {

// //     /// Notifies the action panel of the end of a game.
// //     pub async fn send_game_over(&self, reason: GameOver) {
// //         self.input.send(Input::GameOver(reason)).await.unwrap()
// //     }

// //     /// Notifies the action panel of the start of a game.
// //     pub async fn send_game_start(&self,
// //         starting_cash: u32,
// //         tiles_placed: Vec<Tile>,
// //         play_order: Vec<String>
// //     ) {
// //         self.input.send(
// //             Input::GameStart { starting_cash, tiles_placed, play_order }
// //         ).await.unwrap()
// //     }

// //     /// Requests an action from the panel.
// //     pub async fn request_action(&self, request: ActionRequest) {
// //         self.input.send(Input::YourTurn(request)).await.unwrap()
// //     }

// //     /// Notifies the action panel that it's drawn a tile.
// //     pub async fn send_tile(&self, tile: Tile) {
// //         self.tiles.send(tile).await.unwrap()
// //     }
// // }

// // impl PanelHandler for ActionPanelHandler {

// // }

// // /// Manages the action panel of the client, which is where things truly happen.
// // struct RunningActionPanel {
// //     dim: PanelDim,
// //     rendering_sender: mpsc::Sender<String>,
// //     input: mpsc::Receiver<Input>,
// //     tiles: mpsc::Receiver<Tile>,
// //     player_name: String,
// // }

// // impl RunningActionPanel {

// //     async fn clear(&self) {

// //         // Determine the cursor position
// //         let cursor_x = self.dim.top_left.0 + 2;
// //         let mut cursor_y = self.dim.top_left.1 as u16;

// //         // Send a bunch of rendering messages
// //         futures::future::join_all((0..self.dim.size.1).map(|_| {

// //             self.rendering_sender.send(
// //                 format!("{}{}",
// //                     termion::cursor::Goto(cursor_x, cursor_y),
// //                     " ".repeat(self.dim.size.0 as usize),
// //                 )
// //             )
// //         })).await;

// //         self.rendering_sender.send(
// //             format!("{}", termion::clear::All),
// //         ).await.ok();
// //     }

// //     /// Yielding [`None`] indicates that the client should shut down.
// //     async fn lobby(&mut self) -> Option<(Board, Option<PlayerData>)> {

// //         let mut players: Vec<(String, bool)> = Vec::new();
// //         let mut highlighted_player: usize = 0;

// //         self.clear().await;
        
// //         // Enter the event loop
// //         loop {
// //             let msg = self.input.recv().await?;
    
// //             match msg {
// //                Input::PlayerJoin { name, spectating } => {

// //                     players.push((name, spectating));

// //                     // Rerender
// //                     for (i, (player, spectating)) in players.iter().enumerate() {

// //                         // Start with the goto
// //                         let mut format_str = format!("{}", termion::cursor::Goto(3, 2 + i as u16));

// //                         format_str += "[";

// //                         // Format depending on if this line is highlighted
// //                         if highlighted_player == i {
// //                             format_str += &format!(
// //                                 "{} {}",
// //                                 termion::color::Cyan.fg_str(),
// //                                 termion::color::Reset.fg_str()
// //                             );
// //                         } else {
// //                             format_str += " ";
// //                         }

// //                         // Add the player name
// //                         format_str += &format!("] {}\n", player);
// //                         self.rendering_sender.send(format_str).await.ok();
// //                     }
// //                 }
// //                 Input::GameOver(_) => {
// //                     // Ignore; we're in the lobby
// //                 },
// //                 Input::YourTurn(_) => {
// //                     // Ignore; we're in the lobby
// //                 },
// //                 Input::TilePlacement(_, _) => {
// //                     // Ignore; we're in the lobby
// //                 },
// //                 Input::GameStart {
// //                     starting_cash,
// //                     tiles_placed,
// //                     play_order: player_order, 
// //                 } => {

// //                     // Break out of the lobby
// //                     return Some(self.set_up_game_data(starting_cash, tiles_placed, player_order).await?);
// //                 },
// //             }
// //         }
// //     }

// //     async fn set_up_game_data(&mut self,
// //         starting_cash: u32,
// //         tiles_placed: Vec<Tile>,
// //         player_order: Vec<String>
// //     ) -> Option<(Board, Option<PlayerData>)> {

// //         // Places the tiles on the board
// //         let mut board = Board::new();
// //         for tile in tiles_placed {
// //             board.place_tile(tile, None);
// //         }

// //         let player_order = player_order.iter()

// //             // Is this client spectating?
// //             .enumerate()
// //             .find_map(|(order, name)| {
// //                 if name == &self.player_name { Some(order) }
// //                 else { None }
// //             });

// //         // If it isn't, generate its player data
// //         let player_data = match player_order {
// //             Some(order) => {

// //                 // Wait for tiles
// //                 let mut tiles = [None; 6];

// //                 for i in 0..6 {
// //                     let tile = self.tiles.recv().await?;

// //                     tiles[i] = Some(tile);
// //                 }

// //                 Some(PlayerData {
// //                     money: starting_cash,
// //                     holdings: Default::default(),
// //                     tiles,
// //                     order,
// //                 })
// //             },
// //             None => None,
// //         };

// //         Some((board, player_data))
// //     }

// //     async fn game(&mut self, mut board: Board, data: Option<PlayerData>) -> Option<()> {
        
// //         self.clear().await;

// //         // Enter the event loop
// //         loop {
// //             let msg = self.input.recv().await?;
    
// //             match msg {
// //                 Input::PlayerJoin { name, spectating } => todo!(),
// //                 Input::GameOver(_) => {
                    
// //                     // Go back to the lobby
// //                     return Some(());
// //                 },
// //                 Input::YourTurn(action) => {
// //                     todo!();
// //                 },
// //                 Input::TilePlacement(tile, implication) => {

// //                     match implication {
// //                         TilePlacementImplication::None => {
// //                             board.place_non_merging_tile(tile);
// //                         },
// //                         TilePlacementImplication::FoundsCompany(company) => {
// //                             board.found_company(tile, company);
// //                         },
// //                         TilePlacementImplication::MergesCompanies(merge) => todo!(),
// //                     }

// //                     // Rerender the board
// //                 }
// //                 Input::GameStart {
// //                     starting_cash: _,
// //                     tiles_placed: _,
// //                     play_order: _, 
// //                 } => {
// //                     // Ignore; we're mid-game
// //                 },
// //             }
// //         }
// //     }
// // }

// // #[derive(Debug)]
// // enum Input {
// //     GameOver(GameOver),
// //     GameStart {
// //         starting_cash: u32,
// //         tiles_placed: Vec<Tile>,
// //         play_order: Vec<String>,
// //     },
// //     /// The client needs to deliver on some action.
// //     YourTurn(ActionRequest),
// //     TilePlacement(Tile, TilePlacementImplication),
// //     PlayerJoin {
// //         name: String,
// //         spectating: bool,
// //     }
// // }
