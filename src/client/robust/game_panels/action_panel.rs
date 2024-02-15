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
                let chooser = CompanyChooser::new(&available_companies.true_companies(), true);
                ActionState::BuyingStock {
                    purchases: [None; 3],
                    index: 0,
                    chooser,
                }
            },
            ActionPanelRequest::FoundCompany { tile_placed, available_companies } => {
                let chooser = CompanyChooser::new(&available_companies.true_companies(), false);
                ActionState::FoundingCompany {
                    tile_placed,
                    chooser,
                }
            },
            ActionPanelRequest::Merge { tile_placed, participants } => {
                let chooser = CompanyChooser::new(&participants.true_companies(), false);
                ActionState::Merging {
                    tile_placed,
                    chooser
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
            Some(ActionState::BuyingStock { mut purchases, index, mut chooser}) => {

                let action = match key {
                    // Cycle between the companies to buy stock
                    Key::Left => {
                        chooser.cycle_left();
                        self.action = Some(ActionState::BuyingStock {
                            purchases, index, chooser
                        });
                        None
                    },
                    Key::Right => {
                        chooser.cycle_right();
                        self.action = Some(ActionState::BuyingStock {
                            purchases, index, chooser
                        });
                        None
                    },

                    // Enter advances to the next stock
                    Key::Char('\n') => {
                        purchases[index] = chooser.selected_company();
                        if index == 2 {
                            Some(PlayerAction::BuyStock { stock: purchases })
                        } else {
                            self.action = Some(ActionState::BuyingStock {
                                purchases,
                                index: index + 1,
                                chooser
                            });
                            None
                        }
                    },
                    // Invalid key does nothing
                    _ => {
                        self.action = Some(ActionState::BuyingStock {
                            purchases, index, chooser
                        });
                        None
                    }
                };

                self.render(hand);

                action
            },
            Some(ActionState::FoundingCompany { tile_placed, mut chooser }) => {
                let action = match key {

                    // Cycle between the founding companies
                    Key::Left => {
                        chooser.cycle_left();
                        self.action = Some(ActionState::FoundingCompany {
                            tile_placed, chooser
                        });
                        None
                    },
                    Key::Right => {
                        chooser.cycle_right();
                        self.action = Some(ActionState::FoundingCompany {
                            tile_placed, chooser
                        });
                        None
                    },

                    // Enter to confirm selection
                    Key::Char('\n') => {
                        Some(PlayerAction::PlayTile {
                            placement: TilePlacement {
                                tile: tile_placed,
                                implication: Some(TilePlacementImplication::FoundsCompany(
                                    chooser.selected_company().unwrap()
                                ))
                            }
                        })
                    },

                    // Invalid key does nothing
                    _ => {
                        self.action = Some(ActionState::FoundingCompany {
                            tile_placed, chooser
                        });
                        None
                    }
                };

                self.render(hand);

                action
            },
            Some(ActionState::Merging { tile_placed, chooser }) => {
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
            Some(ActionState::ResolvingMergeStock { selling, keeping, trading, mut highlighted }) => {
                let action = match key {
                    Key::Up => {
                        // Indicates that the up arrow is highlighted
                        highlighted = highlighted % 3 + 3;
                        self.action = Some(ActionState::ResolvingMergeStock { selling, keeping, trading, highlighted });
                        None
                    },
                    Key::Down => {
                        // Indicates that the down arrow is highlighted
                        highlighted = highlighted % 3;
                        self.action = Some(ActionState::ResolvingMergeStock { selling, keeping, trading, highlighted });
                        None
                    },
                    Key::Left => {
                        // Overflow protection
                        if highlighted == 0 { highlighted = 2; }
                        if highlighted == 3 { highlighted = 5; }
                        highlighted -= 1;
                        self.action = Some(ActionState::ResolvingMergeStock { selling, keeping, trading, highlighted });
                        None
                    },
                    Key::Right => {
                        if highlighted == 2 { highlighted = 0; }
                        if highlighted == 5 { highlighted = 3; }
                        highlighted += 1;
                        self.action = Some(ActionState::ResolvingMergeStock { selling, keeping, trading, highlighted });
                        None
                    },
                    Key::Char('\n') => {
                        Some(PlayerAction::ResolveMergeStock { selling, trading, keeping })
                    }
                    _ => None,
                };

                self.render(hand);

                action
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
                        Some(BuyingStock { purchases: _, index, chooser }) => {
                            write_tiles(writer, hand, None, self.tile_layout);
        
                            writer.set_overflow_mode(OverflowMode::Wrap);
        
                            writer.new_line();
                            writer.new_line();
                            writer.write_str("Choose which stock to buy").unwrap();
                            writer.new_line();
                            writer.new_line();
        
                            writer.set_overflow_mode(OverflowMode::Truncate);
        
                            write_company_chooser(
                                writer,
                                chooser.selected_company(),
                            );
                        },
                        Some(FoundingCompany { tile_placed, chooser }) => {
                            write_tiles(writer, hand, None, self.tile_layout);
        
                            writer.write_str("Found which company?").unwrap();

                            write_company_chooser(writer, chooser.selected_company())
                        },
                        Some(Merging { tile_placed: _, chooser}) => {
                            write_tiles(writer, hand, None, self.tile_layout);
        
                            writer.write_str("Choose the company to remain on the board.").unwrap();

                            write_company_chooser(writer, chooser.selected_company())
                        },
                        Some(ResolvingMergeStock { selling, keeping, trading, highlighted }) => {
                            let top_selected = highlighted / 3 == 1;
                            let selected = highlighted % 3;

                            write_number_selector_series(
                                writer, 
                                &[*selling, *keeping, *trading],
                                top_selected,
                                selected as usize
                            );
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

fn write_company_chooser(writer: &mut TermWriter, company: Option<Company>) {

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
    writer.write_str(" >\n").unwrap();
}

/// Writes a series of number selectors in a line.
fn write_number_selector_series(writer: &mut TermWriter,
    values: &[u8],
    top_selected: bool,
    selected: usize
) {
    // Write top row
    for (i, _value) in values.iter().enumerate() {
        writer.write_str("    ").unwrap();
        if top_selected && i == selected {
            writer.write_bg_colored("^^^", termion::color::White).unwrap();
        } else {
            writer.write_str("vvv").unwrap();
        }
    }

    writer.write_str("\n").unwrap();

    // Write row of selectors
    for (_, value) in values.iter().enumerate() {
        writer.write_str("    ").unwrap();
        if value < &100 {
            writer.write_str(" ").unwrap();
        }
        if value < &10 {
            writer.write_str(" ").unwrap();
        }
        writer.write_str(&value.to_string()).unwrap()
    }

    writer.write_str("\n").unwrap();

    // Write bottom row
    for (i, _value) in values.iter().enumerate() {
        writer.write_str("    ").unwrap();
        if !top_selected && i == selected {
            writer.write_bg_colored("vvv", termion::color::White).unwrap();
        } else {
            writer.write_str("vvv").unwrap();
        }
    }

    writer.write_str("\n").unwrap();
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
        /// The purchases made so far
        purchases: [Option<Company>; 3],
        /// Which of the 3 companies is hovered over at the chooser
        index: usize,
        chooser: CompanyChooser,
    },
    FoundingCompany {
        tile_placed: Tile,
        chooser: CompanyChooser,
    },
    Merging {
        tile_placed: Tile,
        /// Chooses the company into which all other companies will consolidate
        chooser: CompanyChooser,
    },
    ResolvingMergeStock {
        selling: u8,
        keeping: u8,
        trading: u8,
        /// Indicates what part of the selection window is highlighted. `% 3`
        /// result: 0 for selling, 1 for keeping, 2 for trading. `/ 3` result
        /// indicates the direction highlighted: 0 for down, 1 for up.
        highlighted: u8,
    }
}

/// Contains the data required to track a company selector box.
#[derive(Debug)]
struct CompanyChooser {
    /// The array of companies included
    included_companies: [Option<Company>; 8],
    /// Specifies which company is being selected.
    selected: i8,
    /// Reference variable for the number of elements being selected
    length: i8,
}

impl CompanyChooser {
    /// Creates a new CompanyChooser, expecting `available_companies`
    pub fn new(available_companies: &[Company], includes_null: bool) -> Self {
        let mut included_companies = [None; 8];
        
        for (i, company) in available_companies.iter().enumerate() {
            included_companies[i] = Some(*company)
        }

        let mut length = available_companies.len() as i8;
        if includes_null { length += 1; }

        debug_assert!(length != 0, "Constructed an empty CompanyChooser");

        Self {
            included_companies,
            selected: 0,
            length,
        }
    }

    pub fn cycle_left(&mut self) {
        self.selected = (self.selected - 1).rem_euclid(self.length);
    }

    pub fn cycle_right(&mut self) {
        self.selected = (self.selected + 1).rem_euclid(self.length);
    }

    pub fn selected_company(&self) -> Option<Company> {
        self.included_companies[self.selected as usize]
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
