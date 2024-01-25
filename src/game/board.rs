use std::collections::HashSet;
use std::ops;

use crate::client::robust::terminal::{TermRender, TermWriteError, TermWriter};

use super::messages::*;
use super::tile::Tile;
use super::{Company, CompanyMap};

/// Contains all the common knowledge pertaining to the game board. This struct
/// is intended for use by both the client and the server.
#[derive(Debug, Clone, Copy)]
pub struct Board {
    /// Arranged in ascending order by number, with the letter breaking ties.
    /// [`None`] represents an empty cell, [`Some(None)`] represents a cell not
    /// belonging to a company. TODO: consider a HashMap?
    tiles: [Option<Option<Company>>; 108],
    /// The sizes of each company on the board
    pub company_sizes: CompanyMap<u8>,
    /// The location of a merger happening on the board, if there is one.
    merger_tile: Option<Tile>,
    /// Locations of headquarters on the board.
    headquarters: CompanyMap<Option<Tile>>,
}

impl Board {
    /// Creates a new empty board.
    pub fn new() -> Self {
        Self {
            tiles: [None; 108],
            company_sizes: Default::default(),
            merger_tile: None,
            headquarters: Default::default(),
        }
    }

    /// Places a tile on the board. This function assumes that the move being
    /// made is legal. If a merge occurs, the board will be left in an
    /// intermediate merging state. It is expected that the caller anticipates
    /// this and calls [`resolve_merge`] in the future.
    pub fn place_tile(&mut self, placement: TilePlacement) {
        match placement.implication {
            None => {
                
                // Places the tile on the board.
                self.place_non_merging_tile(placement.tile);
            },
            Some(TilePlacementImplication::FoundsCompany(company)) => {

                // Create the company on the board
                self.found_company(placement.tile, company);
            },
            Some(TilePlacementImplication::MergesCompanies(_merge)) => {

                // Place the merger tile
                self.place_merger_tile(placement.tile);
            },
        }
    }

    /// Places a tile onto the board, or overwrites an existing tile. Updates
    /// the company sizes accordingly. This function does not check if the move
    /// is legal.
    fn set_tile(&mut self, tile: Tile, affiliation: Option<Company>) {

        let num_cols = Tile::col_as_num(Tile::LAST_COL);
        let pos = &mut self.tiles[
            ((tile.row()-1) * num_cols + (Tile::col_as_num(tile.col())-1)) as usize
        ];
        
        let replaced = std::mem::replace(pos, Some(affiliation));

        // Update company size for the replaced company
        if let Some(replaced) = replaced {
            if let Some(company) = replaced {
                self.company_sizes[company] -= 1;
            }
        }

        // Update company size for the company added to
        if let Some(company) = affiliation {
            self.company_sizes[company] += 1;
        }
    }

    /// Places a tile onto the board, figuring out which chain it should join,
    /// if any.
    /// 
    /// # Panics
    /// 
    /// If the tile being placed would merge two companies, this
    /// function will panic.
    fn place_non_merging_tile(&mut self, tile: Tile) {

        // Look at the surrounding companies to determine affiliation
        let mut affiliation = None;
        self.for_each_neighbor(tile, |neighbor| {

            // Skip empty tiles
            if let Some(neighbor_state) = self[neighbor] {
            
                // If this tile's expected affiliation is different from that of
                // the neighbor's, that means there's a merge.
                if let Some(neighbor_affil) = neighbor_state {
                    if let Some(this_affil) = affiliation {
                        if this_affil != neighbor_affil {
                            panic!("Merger tile {tile} placed as a non-merger");
                        }
                    }

                    affiliation = Some(neighbor_affil);
                }
            }
        });

        // Place the tile
        self.set_tile(tile, affiliation);

        // Assimilate the tile into the company, if needed
        if let Some(company) = affiliation {
            self.update_chain(tile, company);
        }

    }

    /// Places a merger tile onto the board.
    fn place_merger_tile(&mut self, tile: Tile) {
        self.merger_tile = Some(tile);
        self.set_tile(tile, None);
    }

    /// Updates the companies to finish a merge, resolving the companies in
    /// play.
    /// 
    /// # Panics
    /// 
    /// This function assumes the merger tile is already on the board and panics
    /// if that is not the case.
    pub fn resolve_merge(&mut self, merge: &Merge) {
        self.update_chain(
            self.merger_tile
                .expect("Merger tile should already be on the board"),
            merge.into
        );

        // Wipe all the defunct companies from the board
        for defunct in merge.defunct() {
            self.company_sizes[defunct] = 0;
        }

        self.merger_tile = None;
    }

    /// Places the tile and founds a company at the tile. Assumes that this
    /// company doesn't already exist and that this tile is one that legally
    /// founds a company.
    fn found_company(&mut self, tile: Tile, company: Company) {
        self.set_tile(tile, Some(company));
        self.headquarters[company] = Some(tile);
        self.update_chain(tile, company);
    }

    /// Calls a function for each neighbor of the passed tile
    fn for_each_neighbor<F>(&self, tile: Tile, mut f: F)
        where F: FnMut(Tile)
    {
        tile.next_row().map(|tile| f(tile));
        tile.next_col().map(|tile| f(tile));
        tile.prev_row().map(|tile| f(tile));
        tile.prev_col().map(|tile| f(tile));
    }

    /// Calls a function for each neighbor of the passed tile
    fn for_each_neighbor_mut<F>(&mut self, tile: Tile, mut f: F)
        where F: FnMut(&mut Board, Tile)
    {
        tile.next_row().map(|tile| f(self, tile));
        tile.next_col().map(|tile| f(self, tile));
        tile.prev_row().map(|tile| f(self, tile));
        tile.prev_col().map(|tile| f(self, tile));
    }

    /// Updates this tile and all of its neighbors to belong the passed company.
    fn update_chain(&mut self, tile: Tile, company: Company) {

        // Update this tile
        self.set_tile(tile, Some(company));

        self.for_each_neighbor_mut(tile, |board, tile| {
            // Update placed tiles that aren't already part of the company.
            if board[tile].is_some() && board[tile] != Some(Some(company)) {
                // Make a recursive call
                board.update_chain(tile, company);
            }
        });
    }

    /// Checks if a tile is "dead".
    /// 
    /// A company is "safe" if it has more than 10 tiles in play on the board.
    /// Safe companies cannot be merged into other companies. Therefore, any
    /// tile that would merge two safe companies cannot be played and is
    /// considered dead.
    #[inline]
    pub fn dead_tile(&self, tile: Tile) -> bool {
        let mut neighbors: CompanyMap<u8> = Default::default();
        self.for_each_neighbor(tile, |neighbor| {
            if let Some(Some(affil)) = self[neighbor] {
                neighbors[affil] = 1;
            }
        });

        let neighbor_count: u8 = neighbors.into_iter().map(|(_, num)| num).sum();
        neighbor_count > 1
    }

    /// Checks if a tile placement, with its associated implication, is legal.
    pub fn check_implication(&self, placement: TilePlacement) 
        -> Result<(), IncorrectImplication>
    {
        let mut bordering_tiles = HashSet::new();

        self.for_each_neighbor(placement.tile, |neighbor| {
            self[neighbor].map(|c| {
                bordering_tiles.insert(c)
            });
        });

        // See if the tile was placed with any neighboring tiles
        if bordering_tiles.is_empty() {
            
            // There shouldn't be implication
            if &placement.implication != &None {
                return Err(IncorrectImplication::ShouldBeNone)
            }
        }

        let bordering_unaffiliated_tiles = bordering_tiles.remove(&None);
        
        // How many companies are being bordered
        match bordering_tiles.len() {
            // This tile borders no companies
            0 => {
                
                // This tile should found a company
                if bordering_unaffiliated_tiles {

                    // Does the client expect that?
                    if let Some(TilePlacementImplication::FoundsCompany(
                        company
                    )) = &placement.implication {

                        if self.company_exists(*company) {
                            return Err(IncorrectImplication::CompanyTaken);
                        }

                    } else {
                        return Err(IncorrectImplication::ShouldFoundCompany)
                    }
                } else {

                    if !placement.implication.is_none() {
                        return Err(IncorrectImplication::ShouldBeNone)
                    }
                }
            }
            // Tile assimilates into the company
            1 => {
                if !placement.implication.is_none() {
                    return Err(IncorrectImplication::ShouldBeNone);
                }
            }
            // Merger tile
            _ => {

                match &placement.implication {
                    Some(TilePlacementImplication::MergesCompanies(merge)) => {

                        let into_size = self.company_sizes[merge.into];

                        // Ensure only bordering tiles are listed in the defunct
                        for defunct in merge.defunct() {
                            if !bordering_tiles.remove(&Some(defunct)) {
                                return Err(IncorrectImplication::IncorrectDefunct(defunct))
                            }
                        }

                        // Ensure all the bordering companies are listed in the defunct
                        if let Some(missed) = bordering_tiles.into_iter().next() {
                            // Unwrap never panics, as the only None element was removed earlier.
                            return Err(IncorrectImplication::MissedDefunct(missed.unwrap()));
                        }

                        // Ensure all companies are smaller or of equal size
                        if merge.defunct().any(|cmp| self.company_sizes[cmp] > into_size) {
                            return Err(IncorrectImplication::LargeIntoSmall);
                        }

                        // See if any defunct companies are safe
                        if merge.defunct().any(|cmp| self.company_is_safe(cmp)) {
                            return Err(IncorrectImplication::DeadTile);
                        }
                    }
                    _ => return Err(IncorrectImplication::ShouldMerge),
                }
            }
        }

        Ok(())
    }

    /// Determines if a company exists.
    #[inline]
    pub fn company_exists(&self, company: Company) -> bool {
        !(0..=1).contains(&self.company_sizes[company])
    }

    /// Determines if a company is safe, meaning that the company cannot go
    /// defunct for the remainder of the game.
    pub fn company_is_safe(&self, company: Company) -> bool {
        self.company_sizes[company] > 10
    }
    
    /// Gets the stock price per share of a given company. If the company
    /// doesn't exist, zero will be returned.
    pub fn stock_price(&self, company: Company) -> u32 {
        let base_price = match self.company_sizes[company] {
            2 => 200,
            3 => 300,
            4 => 400,
            5 => 500,
            6..=10 => 600,
            11..=20 => 700,
            21..=30 => 800,
            31..=40 => 900,
            41.. => 1000,
            _ => return 0,
        };
        match company {
            Company::Continental | Company::Imperial => base_price + 200,
            Company::American | Company::Festival | Company::Worldwide => base_price + 100,
            Company::Luxor | Company::Tower => base_price,
        }
    }
}

impl TermRender for Board {
    /// This is guaranteed to never fail.
    fn render(&self, term: &mut TermWriter) -> Result<(), TermWriteError> {
        // Write the legend for column numbers
        term.write_str(" 1   5   9   ")?;

        for c in 'a'..=Tile::LAST_COL {

            term.new_line();
            
            // Write the legend for row letters
            if Tile::col_as_num(c) % 2 == 1 {
                term.write_char(c)?;
            } else {
                term.write_char(' ')?;
            }
            
            for r in 1..=Tile::NUM_ROWS {
                let tile = Tile::new(r, c);
                let pos = self[tile];

                // Render the tile normally
                match pos {
                    Some(affiliation) => {
                        match affiliation {
                            Some(company) => {
                                
                                // Determine the character
                                let temp = &[company.char() as u8];
                                let str = if self.headquarters[company] == Some(tile) {
                                    std::str::from_utf8(temp).unwrap()
                                } else if self.company_sizes[company] > 10 {
                                    // Safe company
                                    "o"
                                } else {
                                    // Normal tile
                                    "0"
                                };

                                term.write_bg_colored(str, company)?;
                            },

                            // Unaffiliated tile
                            None => {
                                term.write_str("0")?;
                            },
                        }
                    },

                    // Blank tile.
                    None => {
                        term.write_str(".")?
                    },
                }
            }
        }

        term.new_line();

        Ok(())
    }
}

impl ops::Index<Tile> for Board {
    type Output = Option<Option<Company>>;

    fn index(&self, index: Tile) -> &Self::Output {
        let num_cols = Tile::col_as_num(Tile::LAST_COL);
        &self.tiles[((index.row()-1) * num_cols + (Tile::col_as_num(index.col())-1)) as usize]
    }
}
