use std::fmt;

use crate::server::Handshake;

use super::{Company, CompanyMap};
use super::tile::{Tile, FullHand};

use serde::{Serialize, Deserialize};

/// Messages sent from the server to clients to dictate the happenings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ServerMessage {
    Chat {
        player_name: Box<str>,
        message: Box<str>,
    },
    Join {
        #[serde(flatten)]
        handshake: Handshake,
    },
    Quit {
        #[serde(flatten)]
        handshake: Handshake,
    },
    PlayerMove {
        #[serde(flatten)]
        action: TaggedPlayerAction,
    },
    /// A player drew, or had, a tile that cannot be played and is requesting a new one.
    DeadTile {
        player_name: Box<str>,
        dead_tile: Tile,
    },
    /// A new game has begun. This message is personalized for each player.
    GameStart {
        #[serde(flatten)]
        info: GameStart,
        /// Each player is provided their initial hand through this message.
        /// [`None`] is given to spectators.
        initial_hand: Option<FullHand>,
    },
    /// A company has gone defunct, and principle bonuses are to be paid out.
    CompanyDefunct {
        defunct: Company,
        results: Box<[PrincipleShareholderResult]>,
    },
    /// The game is over
    GameOver {
        #[serde(flatten)]
        reason: GameOver,
        results: Box<[FinalResult]>,
    },
    /// The server is shutting down.
    Shutdown,
    /// Tells a player it's their turn, and requests a specific game action.
    YourTurn {
        #[serde(flatten)]
        request: ActionRequest
    },
    /// Informs the player of a tile that has been added to their hand.
    TileDraw {
        tile: Tile,
    },
    /// An invalid message was sent.
    Invalid {
        #[serde(flatten)]
        reason: InvalidMessageReason
    },
}

/// Information about the start of a game.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameStart {
    pub starting_cash: u32,
    pub tiles_placed: Box<[Tile]>,
    pub play_order: Box<[Box<str>]>,
}

/// history of the entire game, i guess
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameHistory {
    pub start: GameStart,
    pub actions: Box<[TaggedPlayerAction]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinalResult {
    pub place: u8,
    pub player_name: Box<str>,
    pub final_money: u32,
}

impl PartialOrd for FinalResult {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FinalResult {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.final_money.cmp(&other.final_money)
    }
}
                    

impl fmt::Display for FinalResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {} with ${}",
            self.place,
            self.player_name,
            self.final_money
        )
    }
}

/// Information revealed about a player upon a merging of a company. This
/// indicates how much stock the player had in that company, how that compared
/// to other players, and if that player is entitled to a bonus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrincipleShareholderResult {
    pub player: Box<str>,
    /// Number of shares the player has
    pub shares: u8,
    /// Position in shareholding
    pub position: u8,
    /// Prize earned as a principal shareholding bonus, if any.
    pub prize: u32,
}

impl fmt::Display for PrincipleShareholderResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.position {
            1 => write!(f,
                "{}, the principle shareholder with {} shares, receives ${}.",
                self.player,
                self.shares,
                self.prize
            ),
            2 => write!(f,
                "{}, the second-place shareholder with {} shares, receives ${}.",
                self.player,
                self.shares,
                self.prize
            ),
            _ => write!(f, "{} had {} shares.", self.player, self.shares),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ClientMessage {
    TakingTurn(PlayerAction),
    /// A chat message, can be sent by anyone.
    Chat {
        message: Box<str>
    },
    /// The client wishes to replace a dead tile.
    DeadTile {
        dead_tile: Tile,
    },
    /// Administrative commands that have restricted use.
    Admin(AdminCommand),
}

/// An action sent from players to the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaggedPlayerAction {
    pub player_name: Box<str>,
    #[serde(flatten)]
    pub action: PlayerAction,
}

/// Writes this PlayerAction as it would appear in a chat
impl fmt::Display for TaggedPlayerAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", &self.player_name)?;

        match &self.action {
            PlayerAction::PlayTile { placement } => {
                write!(f, " played tile {}", placement.tile)?;
                
                match placement.implication {
                    None => {},
                    Some(TilePlacementImplication::FoundsCompany(company)) => {
                        write!(f, ", founding {}", company)?;
                    },
                    Some(TilePlacementImplication::MergesCompanies(merge)) => {
                        // This is quite ugly...maybe fix this later?
                        let msg = merge.defunct()
                            .map(|cmp| cmp.to_string())
                            .collect::<Vec<_>>().join(" and ");
                        write!(f, ", merging {} into {}", msg, merge.into)?;
                    },
                }
            },
            PlayerAction::BuyStock{ stock } => {

                // Short-circuit for buying nothing
                if stock.iter().all(|p| p.is_none()) {
                    return write!(f, " abstained from buying stock")
                }

                let mut count: CompanyMap<u8> = Default::default();
                stock.iter().for_each(|sale| {
                    sale.map(|sale| count[sale] += 1);
                });
                write!(f, " bought ")?;
                let strings: Vec<String> = count.iter()
                    .filter(|(_, &count)| count > 0)
                    .map(|(company, count)| format!("{} {}", count, company))
                    .collect();
                write!(f, "{}", strings.join(", "))?;
            },
            PlayerAction::ResolveMergeStock { selling, trading, keeping } => {
                let mut strings: Vec<String> = vec![];
                
                if *selling > 0 {
                    strings.push(format!("sold {selling} shares"));
                }

                if *trading > 0 {
                    strings.push(format!("traded in {trading} shares"));
                }

                if *keeping > 0 {
                    strings.push(format!("kept {selling} shares"));
                }

                write!(f, "{}", strings.join(", "))?;
            },
        }
        
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "command")]
pub enum AdminCommand {
    Shutdown,
    StartGame,
    EndGame,
    Kick {
        player_name: Box<str>,
    },
    SilenceChat,
}

/// An action requested from the player by the server.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "request")]
pub enum ActionRequest {
    /// Expected response from the client is [`PlayerAction:PlayTile`].
    PlayTile,
    /// Expected response from the client is [`PlayerAction::BuyStock`].
    BuyStock,
    /// Expected response from the client is [`PlayerAction::ResolveMergeStock`].
    ResolveMergeStock {
        defunct: Company, into: Company
    },
}

/// Indicates an action performed by a player that changes the state of the
/// game.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action")]
pub enum PlayerAction {
    /// A player placed a tile. This is in response to [`ActionRequest::PlayTile`].
    PlayTile {
        #[serde(flatten)]
        placement: TilePlacement
    },
    /// A player bought stock. This is in response to [`ActionRequest::BuyStock`].
    BuyStock {
        /// The stock bought by the player.
        stock: [Option<Company>; 3],
    },
    /// A player resolved his stock in the process of a merger. This is in
    /// response to [`ActionRequest::ResolveMergeStock`].
    ResolveMergeStock {
        selling: u8,
        trading: u8,
        keeping: u8,
    },
}

/// A placement of a tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TilePlacement {
    /// The tile placed.
    pub tile: Tile,
    /// Does the placement of this tile do anything special?
    pub implication: Option<TilePlacementImplication>,
}

/// Indicates anything important that happened during the tile placement
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TilePlacementImplication {
    /// The tile placement founds a new company.
    FoundsCompany(Company),
    /// The tile placement merges two or more companies.
    MergesCompanies(Merge),
}

/// Indicates a merger in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Merge {
    /// The smaller companies that are being removed from the board by the
    /// merging process. 
    /// 
    /// TODO: manually write the [`Deserialize`] implementation so that this is
    /// never in an invalid state.
    defunct: [Option<Company>; 3],
    /// The company into which the defunct company is merging.
    pub into: Company,
}

impl Merge {
    pub fn new(defunct: &[Company], into: Company) -> Self {
        assert!(defunct.len() > 0, "merge created with empty defunct");
        assert!(defunct.len() <= 3,
            "merge created with more than 3 defunct companies"
        );
        let mut defunct_arr= [None; 3];
        for (i, &company) in defunct.iter().enumerate() {
            defunct_arr[i] = Some(company);
        }

        Merge { defunct: defunct_arr, into }
    }

    pub fn defunct(&self) -> impl Iterator<Item = Company> + '_ {
        MergeDefunctIter { iter: self.defunct.iter() }
    }

    pub fn pop_defunct(&mut self) -> Option<Company> {
        for i in (0..3).rev() {
            if self.defunct[i].is_some() {
                return Some(self.defunct[i].take().unwrap());
            }
        }

        None
    }

    pub fn defunct_is_empty(&self) -> bool {
        self.defunct.iter().all(|cmp| cmp.is_none())
    }
}

struct MergeDefunctIter<'a> {
    iter: std::slice::Iter<'a, Option<Company>>,
}

impl<'a> Iterator for MergeDefunctIter<'a> {
    type Item = Company;

    fn next(&mut self) -> Option<Self::Item> {
        match self.iter.next() {
            Some(Some(cmp)) => Some(*cmp),
            Some(None) | None => None,
        }
    }
}

/// Reason why the game ended.
#[derive(Debug, Clone, Copy, thiserror::Error, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "reason")]
pub enum GameOver {
    /// All the stock in the active companies have been bought out.
    #[error("there is no stock left to buy")]
    NoStock,
    /// No legal tile placements can be made.
    #[error("no more tiles can be placed legally")]
    NoTiles,
    /// One company has reached 41 or more tiles in size.
    #[error("{company} is larger than 40 tiles and thus dominates the board")]
    DominatingCompany {
        company: Company
    },
    /// The game was manually ended early.
    #[error("the game was ended by the host")]
    EndedEarly,
}

#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "reason", content = "error")]
pub enum InvalidMessageReason {
    /// A command to start the game was sent to a server where a game is already
    /// underway.
    #[error("a game has already started")]
    GameAlreadyStarted,
    /// A command pertaining to a game was sent to a server where a game is not
    /// in progress.
    #[error("the server is not facilitating a game")]
    NoGameStarted,
    /// The message was valid, but sent out of turn.
    #[error("message sent out of turn")]
    OutOfTurn,
    #[error("an error occurred when buying stock")]
    BuyStockError(#[from] BuyStockError),
    #[error("an error occurred when resolving a merge")]
    MergeResolveError(#[from] MergeResolveError),
    /// A player tried to play or exchange a tile they don't have.
    #[error("player doesn't possess tile")]
    TileNotFound,
    /// The client requested replacement of a tile that wasn't dead.
    #[error("cannot replace tile, as it is not a dead tile")]
    NotDeadTile,
    /// The tile implication is incorrect.
    #[error("incorrect tile implication")]
    IncorrectTileImplication(#[from] IncorrectImplication),
    /// The client does not have the permission needed to send an admin command.
    #[error("cannot send admin command")]
    PermissionDenied,
    /// A message that was sent over JSON was invalid
    #[error("invalid JSON: {0}")]
    JsonParseErr(Box<str>),
}

/// An illegal tile placement move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "reason", content = "company")]
pub enum IncorrectImplication {
    #[error("implication should be none")]
    ShouldBeNone,
    #[error("implication should be founding of company")]
    ShouldFoundCompany,
    /// The company being founded in the implication already exists.
    #[error("company being founded already exists")]
    CompanyTaken,
    #[error("implication should be merge")]
    ShouldMerge,
    #[error("{0} doesn't border the merger tile but was listed as defunct")]
    IncorrectDefunct(Company),
    #[error("{0} bordered the merger tile and was ignored")]
    MissedDefunct(Company),
    /// The tile merges two safe companies, and thus cannot be played.
    #[error("tile cannot be played")]
    DeadTile,
    /// The implied merge illegally attempts to merge a large company into a
    /// smaller one.
    #[error("implication merges large company into small one")]
    LargeIntoSmall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "reason")]
pub enum BuyStockError {
    /// Player attempted to buy stock in a company that's not on the board.
    #[error("company {company} doesn't exist")]
    NonexistentCompany {
        company: Company
    },
    /// The player attempted to buy stock in a company where no more shares
    /// are available.
    #[error("no more shares to purchase")]
    OutOfStock,
    /// The player has an insufficient amount of funds to cover a purchase.
    #[error("lacking ${deficit}")]
    InsufficientFunds {
        deficit: u32,
    }
}

#[derive(Debug, Clone, Copy, thiserror::Error, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeResolveError {
    /// A player attempted to sell, keep or trade stock in a defunct company
    /// that the player did not own.
    #[error("attempted to resolve more stock than possessed")]
    ResolvesNonexistentStock,
    /// Stocks in a defunct company can be traded in for the larger company at a
    /// rate of 2 to 1. The player attempted to trade in an odd number of
    /// stock.
    #[error("traded in an odd number of stock")]
    TradesInOddStock,
    /// The player attempted to trade for stock in a company where no more
    /// shares are available.
    #[error("no more shares to purchase")]
    OutOfStock,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn serialize() {
        let actions = [
            ActionRequest::BuyStock,
            ActionRequest::PlayTile,
            ActionRequest::ResolveMergeStock { defunct: Company::Festival, into: Company::Continental }
        ];
        println!("{}", serde_json::to_string_pretty(&actions).unwrap());

        let commands = [
            AdminCommand::StartGame,
            AdminCommand::SilenceChat,
            AdminCommand::Kick { player_name: "wallaby".to_owned().into_boxed_str() },
            AdminCommand::EndGame,
            AdminCommand::Shutdown,
        ];
        println!("{}", serde_json::to_string_pretty(&commands).unwrap());

        let errors = [
            BuyStockError::InsufficientFunds { deficit: 42 },
            BuyStockError::NonexistentCompany { company: Company::Luxor },
            BuyStockError::OutOfStock,
        ];
        println!("{}", serde_json::to_string_pretty(&errors).unwrap());
        let messages = [
            ClientMessage::Admin(AdminCommand::Kick { player_name: "wallaby".to_owned().into_boxed_str() }),
            ClientMessage::Chat { message: "hello, world!".to_owned().into_boxed_str() },
            ClientMessage::TakingTurn(PlayerAction::ResolveMergeStock { selling: 3, trading: 4, keeping: 6 }),
            ClientMessage::TakingTurn(PlayerAction::PlayTile {
                placement: TilePlacement {
                    tile: Tile::new(3, 'f'), implication: Some(
                        TilePlacementImplication::MergesCompanies(
                            Merge::new(&[Company::Worldwide], Company::Continental)
                        )
                    )
                }
            }),
        ];
        println!("{}", serde_json::to_string_pretty(&messages).unwrap());
        let gameovers = [
            GameOver::DominatingCompany { company: Company::Imperial },
            GameOver::EndedEarly,
        ];
        println!("{}", serde_json::to_string_pretty(&gameovers).unwrap());
        let errors = [
            IncorrectImplication::IncorrectDefunct(Company::American),
            IncorrectImplication::DeadTile,
            IncorrectImplication::ShouldFoundCompany,
        ];
        println!("{}", serde_json::to_string_pretty(&errors).unwrap());
        let invalids = [
            InvalidMessageReason::BuyStockError(
                BuyStockError::NonexistentCompany { company: Company::Luxor }
            ),
            InvalidMessageReason::MergeResolveError(
                MergeResolveError::ResolvesNonexistentStock
            ),
            InvalidMessageReason::JsonParseErr("string".to_owned().into_boxed_str()),
            InvalidMessageReason::OutOfTurn,
        ];
        println!("{}", serde_json::to_string_pretty(&invalids).unwrap());
        let actions = [
            PlayerAction::BuyStock { stock: [None, None, None] },
            PlayerAction::PlayTile { placement: TilePlacement {
                tile: Tile::new(1, 'a'),
                implication: Some(
                    TilePlacementImplication::FoundsCompany(Company::Worldwide)
                ),
            } }
        ];
        println!("{}", serde_json::to_string_pretty(&actions).unwrap());
        let broadcasts = [
            ServerMessage::Chat {
                player_name: "wallaby".to_owned().into_boxed_str(),
                message: "hello".to_owned().into_boxed_str(),
            },
            ServerMessage::Shutdown,
            ServerMessage::GameOver {
                reason: GameOver::DominatingCompany {
                    company: Company::Continental
                },
                results: vec![FinalResult {
                    place: 1,
                    player_name: "wallaby".to_owned().into_boxed_str(),
                    final_money: 42069,
                }].into_boxed_slice(),
            },
            ServerMessage::PlayerMove {
                action: TaggedPlayerAction {
                    player_name: "wallaby".to_owned().into_boxed_str(),
                    action: PlayerAction::PlayTile {
                        placement: TilePlacement {
                            tile: Tile::new(6, 'i'),
                            implication: None
                        }
                    }
                }
            }
        ];
        println!("{}", serde_json::to_string_pretty(&broadcasts).unwrap());
    }
}
