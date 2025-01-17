use termion::event::Key;

use crate::game::{board::Board, tile::{Hand, Tile, TileHand, HAND_SIZE}};
use crate::client::robust::panels::{PanelDim, PanelTooSmallError};
use crate::client::robust::terminal::{NiceFgColor, OverflowMode, TermPanel, TermWriter};

use super::{IncorrectImplication, TextPanel, TilePlacement};

/// Manages the tile display of the action panel, as well as a cache of the hand
/// the player has. The action panel doesn't actually know what's going on in
/// the game, so it stores a cache of the hand it's been told we have.
#[derive(Debug)]
pub struct TilePanel {
    tile_panel: TermPanel,
    /// Stored even if this panel is inactive to preserve the column.
    highlighted_index: usize,
    /// Indicates whether the user is hovering over this panel (for rendering
    /// purposes).
    highlighted: bool,
    desc_panel: TextPanel,
    layout: TileLayout,
    hand_cache: AnnotatedHand,
}

pub enum TilePanelKeyProcessEvent {
    /// The event resulted in the completion of a player action.
    TileChosen {
        chosen: Tile,
        cached_annotation: Option<IncorrectImplication>,
    },
    /// The player moved their cursor upwards to exit the panel.
    ExitUpward,
    /// The player moved their cursor downwards to exit the panel.
    ExitDownward,
}

impl TilePanel {

    fn cut_panels(mut panel: TermPanel) -> Result<(TermPanel, TermPanel), PanelTooSmallError> {
        let (_, desc) = panel.shave_vert(0, 2)?;
        Ok((panel, desc))
    }

    pub fn new(panel: TermPanel) -> Result<Self, PanelTooSmallError> {
        let tile_layout = TileLayout::decide(panel.dim())?;
        let (main, desc) = Self::cut_panels(panel)?;

        let mut me = Self {
            tile_panel: main,
            desc_panel: TextPanel::new(desc),
            layout: tile_layout,
            hand_cache: AnnotatedHand::default(),
            highlighted: false,
            highlighted_index: 0,
        };
        me.rerender();
        Ok(me)
    }

    pub fn resize(&mut self, new_panel: TermPanel) -> Result<(), PanelTooSmallError> {
        let tile_layout = TileLayout::decide(new_panel.dim())?;
        let (main, desc) = Self::cut_panels(new_panel)?;
        self.desc_panel = TextPanel::new(desc);
        self.tile_panel = main;
        self.layout = tile_layout;
        self.rerender();
        Ok(())
    }

    /// Updates this panel's cache with a new hand.
    pub fn provide_new_hand(&mut self, hand: TileHand, board: &Board) {
        self.hand_cache = AnnotatedHand::new(hand, board);
        self.rerender();
    }

    /// Updates the board state, which may change the playability status of the
    /// tiles in the cached hand, without changing the hand itself.
    pub fn update_board_state(&mut self, board: &Board) {
        self.hand_cache.reannotate(board);
    }

    /// Provides the hand cache with a newly drawn tile.
    pub fn provide_tile(&mut self, new_tile: Tile, board: &Board) -> Result<(), Tile> {
        self.hand_cache.insert_tile(new_tile, board)
    }

    /// Removes a tile from the player's hand
    pub fn remove_tile(&mut self, tile: Tile) -> bool {
        self.hand_cache.remove_tile(tile)
    }

    /// Informs the panel that it has been highlighted by the user, for rendering purposes.
    pub fn highlight(&mut self) {
        self.highlighted = true;
        self.rerender();
    }

    /// Informs the panel that the cursor has jumped out of the panel, for rendering purposes.
    pub fn unhighlight(&mut self) {
        self.highlighted = false;
        self.rerender();
    }

    pub fn process_key(&mut self, key: Key) -> Option<TilePanelKeyProcessEvent> {

        let offset = match key {
            Key::Left => Some((-1, false)),
            Key::Right => Some((1, false)),
            Key::Up => Some((-(self.layout.vertical_count() as i8), true)),
            Key::Down => Some((self.layout.vertical_count() as i8, true)),
            Key::Char('\n') => None,
            _ => return None,
        };

        // We only reach here if a key we care about is processed.
        match offset {
            // Arrow key was pressed
            Some((offset, is_vertical)) => {
                let new_idx = self.highlighted_index as i8 + offset;

                // Handle tile panel exit cases
                if is_vertical {
                    if new_idx > HAND_SIZE as i8 {
                        return Some(TilePanelKeyProcessEvent::ExitDownward);
                    }
                    else if new_idx < 0 {
                        return Some(TilePanelKeyProcessEvent::ExitUpward);
                    }
                }

                // Mod by HAND_SIZE to keep the index within the panel bounds
                // (this leads to side wrapping)
                self.highlighted_index = new_idx.rem_euclid(HAND_SIZE as i8) as usize;

                self.rerender();

                None
            },
            // Enter was pressed, so we emit the tile chosen
            None => {

                // Ensure the tile chosen is Some; otherwise, don't emit an action
                let Some(tile) = self.hand_cache.get_tile(self.highlighted_index) else {
                    return None;
                };

                self.rerender();

                Some(TilePanelKeyProcessEvent::TileChosen {
                    chosen: tile,
                    cached_annotation: self.hand_cache.get_annotation(self.highlighted_index)
                })
            },
        }
    }

    fn rerender_tiles(&mut self) {
        let tiles = self.hand_cache.annotated_hand.map(|entry| entry.tile);

        // Print the tiles themselves
        self.tile_panel.clear();
        self.tile_panel.write(OverflowMode::Truncate, |writer| {
            match self.layout {
                TileLayout::Grid2x3 => {
                    for row in 0..2 {
                        for col in 0..3 {
                            let idx = row * 3 + col;
                            let selected = self.highlighted && idx == self.highlighted_index;
                            write_tile(writer, tiles.tiles()[idx], selected);
                            writer.write_str("  ").unwrap();
                        }
                        writer.new_line();
                    }
                },
                TileLayout::Grid3x2 => {
                    for row in 0..3 {
                        for col in 0..2 {
                            let idx = row * 2 + col;
                            let selected = self.highlighted && idx == self.highlighted_index;
                            write_tile(writer, tiles.tiles()[idx], selected);
                        }
                        writer.new_line();
                    }
                },
                TileLayout::Column => {
                    for row in 0..6 {
                        let selected = self.highlighted && row == self.highlighted_index;
                        write_tile(writer, tiles.tiles()[row], selected);
                    }
                },
            }
        });
    }

    fn rerender(&mut self) {

        self.rerender_tiles();

        if self.highlighted {
            let (msg, color) = self.hand_cache.get_message(self.highlighted_index);
            self.desc_panel.display_text(&msg, &*color).unwrap();
        }
    }
}

/// A hand, annotated with information about the playability of each tile.
#[derive(Debug, Default, Clone, Copy)]
struct AnnotatedHand {
    can_found_companies: bool,
    annotated_hand: Hand<AnnotatedHandEntry>,
}

#[derive(Debug, Clone, Copy)]
struct AnnotatedHandEntry {
    tile: Tile,
    /// If the tile were to be played with implication [`None`], this would be
    /// the error returned.
    playability: Option<IncorrectImplication>,
}

/// Equality concerns just the tile, not the playability hand annotation
impl PartialEq for AnnotatedHandEntry {
    fn eq(&self, other: &Self) -> bool {
        self.tile == other.tile
    }
}

impl Eq for AnnotatedHandEntry {}

impl AnnotatedHandEntry {
    pub fn new(tile: Tile, board: &Board) -> Self {
        let implication_check = board.check_implication(
            TilePlacement {
                tile,
                implication: None
            }
        );

        let playability = match implication_check {
            Ok(()) => None,
            Err(e) => Some(e),
        };

        Self {
            tile,
            playability,
        }
    }
}

impl AnnotatedHand {
    pub fn new(hand: TileHand, board: &Board) -> Self {

        // Can found companies only if there's at least one company that doesn't exist (size is 0)
        let can_found_companies = Self::compute_can_found_companies(board);

        // Annotate each tile in the hand
        let annotated_hand = hand.map(|tile| AnnotatedHandEntry::new(tile, board));

        Self {
            can_found_companies,
            annotated_hand,
        }
    }

    fn compute_can_found_companies(board: &Board) -> bool {
        !board.company_sizes.iter().all(|(_company, size)| { *size > 0 })
    }

    /// Changes the hand annotations without changing the hand itself.
    pub fn reannotate(&mut self, board: &Board) {
        // Reannotate the hand
        self.annotated_hand.tiles_iter_mut()
            .for_each(|entry| *entry = AnnotatedHandEntry::new(entry.tile, board));

        // Update can_found_companies
        self.can_found_companies = Self::compute_can_found_companies(board);
    }

    pub fn insert_tile(&mut self, tile: Tile, board: &Board) -> Result<(), Tile> {

        self.annotated_hand.insert_tile(AnnotatedHandEntry {
            tile,
            // This playability annotation will just be discarded once we call reannotate below
            playability: None,
        }).map_err(|entry| entry.tile)?;

        self.reannotate(board);

        Ok(())
    }

    pub fn remove_tile(&mut self, tile: Tile) -> bool {
        self.annotated_hand.remove_tile(AnnotatedHandEntry {
            tile,
            // This playability annotation doesn't matter, as the provided entry
            // is only used as a compare base (and playability doesn't impact equality)
            playability: None,
        })
    }

    /// Gets the tile at a specified index.
    ///
    /// # Panics
    ///
    /// Panics upon index out of bounds.
    pub fn get_tile(&self, tile_index: usize) -> Option<Tile> {
        Some(self.annotated_hand.tiles()[tile_index]?.tile)
    }

    pub fn get_annotation(&self, tile_index: usize) -> Option<IncorrectImplication> {
        self.annotated_hand.tiles()[tile_index]?.playability
    }

    /// Gets the message and color to print for a tile.
    ///
    /// # Panics
    ///
    /// Panics upon index out of bounds.
    pub fn get_message(&self, tile_index: usize) -> (String, &'static dyn NiceFgColor) {
        let entry = self.annotated_hand.tiles()[tile_index];

        let Some(entry) = entry else {
            return (String::new(), &termion::color::Reset);
        };

        match entry.playability {
            Some(x) => match x {
                IncorrectImplication::ShouldBeNone => {
                    panic!("Shouldn't happen, as we passed an implication of None")
                },
                IncorrectImplication::ShouldFoundCompany => {
                    (
                        if self.can_found_companies {
                            format!("Found company with tile {}", entry.tile)
                        } else {
                            "Can't found company right now".to_owned()
                        },
                        &termion::color::Yellow
                    )
                },
                IncorrectImplication::CompanyTaken => {
                    panic!("Shouldn't happen, as we passed an implication of None")
                }
                IncorrectImplication::ShouldMerge => {
                    (
                        format!("Play merger tile {}", entry.tile),
                        &termion::color::LightGreen
                    )
                },
                IncorrectImplication::IncorrectDefunct(_) => {
                    panic!("Shouldn't happen, as we passed an implication of None")
                },
                IncorrectImplication::MissedDefunct(_) => {
                    panic!("Shouldn't happen, as we passed an implication of None")
                },
                IncorrectImplication::DeadTile => {
                    (
                        "Tile is dead; press Enter to replace".to_owned(),
                        &termion::color::LightRed
                    )
                },
                IncorrectImplication::LargeIntoSmall => {
                    panic!("Shouldn't happen, as we passed an implication of None")
                },
                IncorrectImplication::BadDefunctOrder => {
                    panic!("Shouldn't happen, as we passed an implication of None")
                },
            }
            None => {
                (
                    format!("Play tile {}", entry.tile),
                    &termion::color::Reset
                )
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TileLayout {
    /// Preferred option; 2 rows, 3 columns.
    Grid2x3,
    /// 3 rows, 2 columns.
    Grid3x2,
    /// 6 rows, 1 column.
    Column,
}

impl TileLayout {
    // [ ] X-XX  [ ] X-XX
    // [ ] X-XX  [ ] X-XX
    // [ ] X-XX  [ ] X-XX
    const MIN_WIDTH_3X2: u16 = 8*2 + 2;
    // [ ] X-XX  [ ] X-XX  [ ] X-XX
    // [ ] X-XX  [ ] X-XX  [ ] X-XX
    const MIN_WIDTH_2X3: u16 = 8*3 + 4;
    // [ ] X-XX
    const MIN_WIDTH: u16 = 8;

    /// When the down arrow or up arrow are pressed, this is the number of
    /// indices to advance.
    pub fn vertical_count(&self) -> u8 {
        match self {
            TileLayout::Grid2x3 => 3,
            TileLayout::Grid3x2 => 2,
            TileLayout::Column => 1,
        }
    }

    /// Chooses the best tile layout for a provided set of panel dimensions.
    pub fn decide(dim: PanelDim) -> Result<Self, PanelTooSmallError> {
        if dim.size.0 >= Self::MIN_WIDTH_2X3 && dim.size.1 >= 2 {Ok(Self::Grid2x3) }
        else if dim.size.0 >= Self::MIN_WIDTH_3X2 && dim.size.1 >= 3 { Ok(Self::Grid3x2) }
        else if dim.size.0 >= Self::MIN_WIDTH && dim.size.1 >= 6 { Ok(Self::Column) }
        else { Err(PanelTooSmallError) }
    }
}

fn write_tile(writer: &mut TermWriter, tile: Option<Tile>, selected: bool) {

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
    if let Some(tile) = tile {
        writer.write(&&*tile.to_string()).unwrap();

        // Determine if an extra space needs to be typed
        if tile.row() < 10 {
            writer.write_char(' ').unwrap();
        }
    } else {
        writer.write_str("----").unwrap();
    }
}
