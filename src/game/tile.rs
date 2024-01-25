use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tile {
    row: u8,
    col: u8,
}

impl Tile {

    pub const NUM_ROWS: u8 = 12;
    pub const LAST_COL: char = 'i';

    pub fn new(row: u8, col: char) -> Self {

        if row > Self::NUM_ROWS {
            panic!("Invalid row number. Expected a number between 1 and {}, got {}", Self::NUM_ROWS, row);
        }
        if col < 'a' || col > Self::LAST_COL {
            panic!("Expected a lowercase letter between 'a' and '{}', got {}", Self::LAST_COL, col);
        }

        Self { row, col: col as u8 }
    }

    pub fn row(&self) -> u8 {
        self.row
    }

    pub fn col(&self) -> char {
        self.col as char
    }

    pub fn boneyard() -> Boneyard<Self> {
        let mut boneyard = Vec::with_capacity(
            Self::NUM_ROWS as usize * Self::col_as_num(Self::LAST_COL) as usize
        );

        for row in 1..=Self::NUM_ROWS {
            for col in 'a'..=Self::LAST_COL {
                boneyard.push(Tile::new(row, col));
            }
        }

        Boneyard::new(boneyard)
    }

    pub fn col_as_num(chr: char) -> u8 {
        (chr as u8) - ('a' as u8) + 1
    }

    /// Gets the tile bordering this one in the next row, returning [`None`] if
    /// it doesn't exist.
    pub fn next_row(&self) -> Option<Tile> {
        if self.row == Self::NUM_ROWS { None }
        else { Some(Tile { row: self.row + 1, col: self.col }) }
    }

    /// Gets the tile bordering this one in the previous row, returning [`None`] if
    /// it doesn't exist.
    pub fn prev_row(&self) -> Option<Tile> {
        if self.row == 1 { None }
        else { Some(Tile { row: self.row - 1, col: self.col }) }
    }

    /// Gets the tile bordering this one in the next column, returning [`None`] if
    /// it doesn't exist.
    pub fn next_col(&self) -> Option<Tile> {
        if self.col == (Self::LAST_COL as u8) { None }
        else { Some(Tile { row: self.row, col: self.col + 1 }) }
    }

    /// Gets the tile bordering this one in the previous column, returning [`None`] if
    /// it doesn't exist.
    pub fn prev_col(&self) -> Option<Tile> {
        if self.col == ('a' as u8) { None }
        else { Some(Tile { row: self.row, col: self.col as u8 - 1 }) }
    }
}

impl fmt::Display for Tile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.row, self.col as char)
    }
}

impl PartialOrd for Tile {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Tile {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.row.cmp(&other.row).then(self.col.cmp(&other.col))
    }
}

impl FromStr for Tile {
    type Err = TileFromStrError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {

        use TileFromStrError::*;

        let mut split = s.split("-");
        let row_str = split.next().ok_or(NoDash)?;
        let col_str = split.next().ok_or(NoDash)?;
        if split.next().is_some() { return Err(TwoDashes); }

        let row = row_str.parse()?;
        if !(1..=Self::NUM_ROWS).contains(&row) {
            return Err(InvalidRow(row));
        }

        // The column will be the first byte
        let mut col_chars = col_str.chars();
        let col = match col_chars.next() {
            Some(x) => match x {
                col @ 'a'..='i' => col,
                col @ 'A'..='I' => col.to_ascii_lowercase(),
                _ => return Err(InvalidColumn(col_str.to_owned())),
            },
            None => return Err(InvalidColumn(col_str.to_owned())),
        };
        
        Ok(Tile { row, col: col as u8 })
    }
}

impl serde::Serialize for Tile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for Tile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>
    {
        let string: &str = Deserialize::deserialize(deserializer)?;
        string.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TileFromStrError {
    #[error("expected \"-\"")]
    NoDash,
    #[error("found two dashes in tile")]
    TwoDashes,
    #[error("error parsing row: {0}")]
    ErrorParsingRow(#[from] std::num::ParseIntError),
    #[error("invalid row: {0}")]
    InvalidRow(u8),
    #[error("invalid column: {0}")]
    InvalidColumn(String),
}

/// Represents a player's hand of tiles.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Hand {
    tiles: [Option<Tile>; 6]
}

impl Hand {
    /// Creates a new hand by drawing tiles from a [`Boneyard`]. If the boneyard
    /// runs out of tiles before drawing all of the hand, [`Err`] with a
    /// partially-filled [`Hand`] instance will be returned instead.
    pub fn from_boneyard(boneyard: &mut Boneyard<Tile>) -> Result<FullHand, Hand> {
        let hand = Self { tiles: [(); 6].map(|()| boneyard.remove()) };
        FullHand::try_from(hand).map_err(|_| hand)
    }

    /// Checks if the hand is empty.
    pub fn is_empty(&self) -> bool {
        self.tiles.iter().all(|t| t.is_none())
    }

    /// Counts the number of tiles in the player's hand.
    pub fn len(&self) -> u8 {
        self.tiles.iter()
            .map(|t| if t.is_some() { 1 } else { 0 })
            .sum()
    }

    /// Checks if the hand is full. This function is no faster than attempting
    /// to create a [`FullHand`].
    pub fn is_full(&self) -> bool {
        self.tiles.iter().all(|t| t.is_some())
    }

    pub fn iter(&self) -> impl Iterator<Item = &Tile> {
        self.tiles.iter().filter_map(|t| t.as_ref())
    }

    /// Attempts to swap a tile equivalent to the `old_tile` with the provided
    /// `new_tile`. If the hand has more than one `old_tile`, only one will be
    /// replaced. If the `old_tile` does not exist, the new tile will be
    /// returned rather than inserted.
    pub fn swap_tile(&mut self,
            old_tile: Option<Tile>,
            new_tile: Option<Tile>
    ) -> Result<(), Option<Tile>> {
        for tile in &mut self.tiles {
            if *tile == old_tile {
                *tile = new_tile;
                return Ok(());
            }
        }
        Err(new_tile)
    }

    /// Inserts a tile into an empty slot in a player's hand. If there is no
    /// empty slot, the new tile will be returned rather than inserted.
    #[inline]
    pub fn insert_tile(&mut self, new_tile: Tile) -> Result<(), Tile> {
        self.swap_tile(None, Some(new_tile)).map_err(|e| e.unwrap())
    }

    /// Removes a tile from a player's hand. Returns `true` upon success, and
    /// `false` if the desired tile cannot be found in the hand.
    #[inline]
    pub fn remove_tile(&mut self, tile: Tile) -> bool {
        self.swap_tile(Some(tile), None).is_ok()
    }
}

impl Serialize for Hand {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer
    {
        self.tiles.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Hand {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>
    {
        Ok(Self { tiles: Deserialize::deserialize(deserializer)? })
    }
}

/// Represents a player's hand of tiles, but makes the guarantee that the hand
/// is full. These are constructed by either converting from a [`Hand`], or
/// drawing from a [`Boneyard`] of tiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FullHand {
    tiles: [Tile; 6]
}

impl FullHand {
    /// Attempts to swap a tile equivalent to the `old_tile` with the provided
    /// `new_tile`. If the hand has more than one `old_tile`, only one will be
    /// replaced. Returns `true` upon success, and `false` if the `old_tile`
    /// could not be found in the hand.
    pub fn swap_tile(&mut self, old_tile: Tile, new_tile: Tile) -> Result<(), Tile> {
        for tile in &mut self.tiles {
            if *tile == old_tile {
                *tile = new_tile;
                return Ok(());
            }
        }
        Err(new_tile)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Tile> {
        self.tiles.iter()
    }
}

impl fmt::Display for FullHand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}, {}, {}, {}, {}, and {}",
            self.tiles[0], self.tiles[1],
            self.tiles[2], self.tiles[3],
            self.tiles[4], self.tiles[5],
        )
    }
}

impl Serialize for FullHand {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer
    {
        self.tiles.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for FullHand {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>
    {
        Ok(Self { tiles: Deserialize::deserialize(deserializer)? })
    }
}

impl From<FullHand> for Hand {
    fn from(value: FullHand) -> Self {
        Self { tiles: value.tiles.map(|t| Some(t)) }
    }
}

/// Attempts to convert from a [`Hand`] to a [`FullHand`]. On failure, the
/// number of tiles in the hand will be returned.
impl TryFrom<Hand> for FullHand {
    type Error = u8;

    fn try_from(value: Hand) -> Result<Self, Self::Error> {
        match value.is_full() {
            true => {
                // All values are now guaranteed to be `Some`.
                Ok(Self { tiles: value.tiles.map(|t| t.unwrap()) })
            },
            false => {
                Err(value.len())
            },
        }
    }
}

/// A collection from which items are removed at random.
#[derive(Debug)]
pub struct Boneyard<T> {
    boneyard: Vec<T>,
}

impl<T> Boneyard<T> {
    pub fn new(initial: Vec<T>) -> Self {
        Self { boneyard: initial }
    }

    /// Takes a random value from the boneyard. Returns [`None`] if the boneyard
    /// is empty.
    pub fn remove(&mut self) -> Option<T> {

        // Get a random index
        let idx: usize = rand::random();
        let idx = idx % self.boneyard.len();

        let last = self.boneyard.pop()?;

        if idx == self.boneyard.len() {
            Some(last)
        } else {
            Some(std::mem::replace(&mut self.boneyard[idx], last))
        }
    }
}

#[cfg(test)]
mod test {
    use super::{Tile, TileFromStrError};

    #[test]
    fn tile_parsing() {
        assert_eq!("12-I".parse(), Ok(Tile::new(12, 'i')));
        assert_eq!("12-i".parse(), Ok(Tile::new(12, 'i')));
        assert_eq!("17-i".parse::<Tile>(), Err(TileFromStrError::InvalidRow(17)));
        assert_eq!("i".parse::<Tile>(), Err(TileFromStrError::NoDash));
        assert_eq!("".parse::<Tile>(), Err(TileFromStrError::NoDash));
    }
}
