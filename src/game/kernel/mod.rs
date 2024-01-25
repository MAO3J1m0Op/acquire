use std::{collections::HashMap, sync::atomic::Ordering};
use std::sync::atomic::AtomicUsize;

use crate::game::messages::*;

use self::kernel::GameKernel;

use super::board::Board;
use super::CompanyMap;

mod kernel;
/// Declares everything surrounding the state [`DrawingInitialHands`].
mod init;
/// Declares everything surrounding the state [`PlacingTile`].
mod place_tile;
/// Declares everything surrounding the states [`MaybeResolvingMerge`] and [`ResolvingMerge`].
mod resolve_merge;
/// Declares everything surrounding the state [`BuyingStock`].
mod buy_stock;
/// Declares everything surrounding the state [`Ambiguous`].
mod ambiguous;

pub use {
    place_tile::{
        PlacingTile,
        PlacingTileStateAdvance
    },
    resolve_merge::{
        ContinueMerging,
        DoneMerging,
        MaybeResolvingMerge,
        MergeResolution,
        MergeStep,
        ResolvingMerge
    },
    buy_stock::{
        BuyingStock,
        BuyingStockStateAdvance
    },
    ambiguous::{
        Ambiguous,
        GameDisambiguation
    }
};

/// Controls a game. The game moves between six states, indicated by the type
/// argument. The game is created in the [`DrawingInitialHands`] state, where
/// the game accepts the initial hands of all the players. The game then cycles
/// between up to four states every turn, beginning with [`PlacingTile`] and
/// then moving to [`MaybeResolvingMerge`]. In this state, the game may enter
/// the [`ResolvingMerge`] state. Afterward, the turn ends in the
/// [`BuyingStock`] state. If the game ends, either naturally or forcefully, the
/// game will finish in the [`GameOver`] state. This allows for retrieval of the
/// final state of the board before results are tallied.
#[derive(Debug, Clone)]
pub struct Game<S: State> {
    data: Box<GameImpl>,
    /// The current state of the game.
    state: S,
}

#[derive(Debug)]
struct GameImpl {
    /// Holds the game data itself.
    kernel: GameKernel,
    /// The player who is playing in the current state.
    player: Box<str>,
    /// Unique identifier for this game, used to guard against state transitions
    /// being used for the wrong game.
    id: usize,
}

impl GameImpl {
    pub fn new(kernel: GameKernel, first_player: Box<str>) -> Self {
        Self {
            kernel, 
            player: first_player,
            id: GAME_ID.fetch_add(1, Ordering::Relaxed)
        }
    }
}

impl Clone for GameImpl {
    fn clone(&self) -> Self {
        Self {
            id: GAME_ID.fetch_add(1, Ordering::Relaxed),
            kernel: self.kernel.clone(),
            player: self.player.clone(),
        }
    }
}

impl<S: State> Game<S> {
    /// Gets a reference to the state of the board.
    pub fn board(&self) -> &Board {
        &self.data.kernel.board
    }

    /// Gets a reference to the number of stocks that have been bought from each
    /// company.
    pub fn stock_bank(&self) -> &CompanyMap<u8> {
        &self.data.kernel.stock_bank
    }

    /// Gets a reference to the players and their data.
    pub fn players(&self) -> &HashMap<Box<str>, PlayerData> {
        &self.data.kernel.players
    }

    /// Gets the name of the player from which action is needed.
    pub fn active_player(&self) -> &str {
        &self.data.player
    }

    /// Immediately ends this game with the reason [`GameOver::EndedEarly`].
    pub fn end_early(self) -> Game<GameOver> {
        Game {
            data: self.data,
            state: GameOver::EndedEarly,
        }
    }

    /// Get the current standings of the game.
    #[inline]
    pub fn get_standings(&self) -> Vec<FinalResult> {
        self.data.kernel.get_standings()
    }
}



/// Data about a specific player in the [`Game`].
#[derive(Debug, Clone)]
pub struct PlayerData {
    pub money: u32,
    pub holdings: CompanyMap<u8>,
    pub order: usize,
    pub next_player: Box<str>,
}

/// Creates the IDs of games when initialized.
static GAME_ID: AtomicUsize = AtomicUsize::new(0);
/// This is the panic message if the ID of the object used to advance the state
/// was not equal to the ID of the [`Game`] whose state is being advanced.
const ID_CHECK_FAIL: &str = "argument used to advance state was not created by this object";

impl Game<GameOver> {
    /// Provides the reason that this game is over.
    pub fn reason(&self) -> &GameOver {
        &self.state
    }

    /// Tallies up the final results of the game.
    pub fn tally_results(self) -> GameResults {

        let mut data = self.data;

        let shareholder_results: CompanyMap<_> = data.kernel.board.company_sizes
            .map(|company, _size| {
                let results = data.kernel
                    .pay_principle_bonuses(company)
                    .into_boxed_slice();

                // Sell every player's remaining stock
                for (_player, player_data) in &mut data.kernel.players {
                    player_data.money += data.kernel.board.stock_price(company) 
                        * player_data.holdings[company] as u32;
                }

                if data.kernel.board.company_exists(company) {
                    Some(results)
                } else {
                    None
                }
            });

        let final_standings = data.kernel.get_standings();

        GameResults {
            shareholder_results,
            final_standings: final_standings.into_boxed_slice(),
        }
    }
}

/// The final results of the game.
pub struct GameResults {
    /// Stores the final principle shareholder results for every company that
    /// was on the board at the end of the game.
    pub shareholder_results: CompanyMap<Option<Box<[PrincipleShareholderResult]>>>,
    /// The players in order of placement, with the winner being in index 0.
    /// The [`u32`] indicates the amount of money with which the player finished
    /// the game.
    pub final_standings: Box<[FinalResult]>,
}

/// Result of a successful game update that captures both the new game instance
/// and the possibility of the game ending.
pub type GameUpdateResult<S> = Result<Game<S>, Game<GameOver>>;
/// Result of a tried game update, with the [`Err`] capturing the reason the
/// update was not possible.
pub type TryGameUpdateResult<SNew, SOld> = Result<GameUpdateResult<SNew>, (Game<SOld>, InvalidMessageReason)>;

mod sealed {
    pub trait SealedState {}
    impl SealedState for super::PlacingTile {}
    impl SealedState for super::BuyingStock {}
    impl SealedState for super::MaybeResolvingMerge {}
    impl SealedState for super::ResolvingMerge {}
    impl SealedState for super::Ambiguous {}
    impl SealedState for crate::game::messages::GameOver {}
}

pub trait State: std::fmt::Debug + Clone + sealed::SealedState {}

#[cfg(test)]
mod test {
    use crate::game::Company;
    use crate::game::messages::{
        TilePlacementImplication,
        BuyStockError,
        TilePlacement,
        GameStart
    };
    use crate::game::tile::Tile;

    use super::Game;
    
    #[test]
    pub fn client_side_game() {

        let play_order = vec![
            "player1".to_owned().into_boxed_str(),
            "player2".to_owned().into_boxed_str(),
        ].into_boxed_slice();

        let game = Game::start(&GameStart {
            starting_cash: 6000,
            play_order,
            tiles_placed: vec![Tile::new(3, 'b'), Tile::new(2, 'a')].into_boxed_slice()
        });

        assert_eq!(&*game.active_player(), "player1");

        // Turn 1, no company
        let advancer = game.check_tile(TilePlacement {
            tile: Tile::new(12, 'd'),
            implication: None
    }).unwrap();
        let game = game.advance_game(advancer);
        let decision = game.decide_merge().unwrap();
        let game = game.skip_merge(decision);
        let advancer = game.check_buy_stock([None, None, None]).unwrap();
        assert_eq!(
            game.check_buy_stock([Some(Company::American), None, None]),
            Err(BuyStockError::NonexistentCompany { company: Company::American })
        );
        let game = game.advance_game(advancer).unwrap();

        // Turn 2: found continental
        dbg!(game.board()[Tile::new(3, 'b')]);
        assert_eq!(&*game.active_player(), "player2");
        let advancer = game.check_tile(TilePlacement {
            tile: Tile::new(2, 'b'),
            implication: Some(TilePlacementImplication::FoundsCompany(
                Company::Continental
            ))
        }).unwrap();
        let game = game.advance_game(advancer);
        let decision = game.decide_merge().unwrap();
        let game = game.skip_merge(decision);
        let advancer = game.check_buy_stock(
            [Some(Company::Continental), None, None]
        ).unwrap();
        let game = game.advance_game(advancer).unwrap();

        assert_eq!(game.stock_bank()[Company::Continental], 1);
        assert_eq!(&*game.active_player(), "player1");
    }
}
