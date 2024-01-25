use crate::game::messages::*;

use super::{Game, place_tile::PlacingTile, resolve_merge::{ResolvingMerge, MaybeResolvingMerge}, buy_stock::BuyingStock, State, TryGameUpdateResult};

/// A state that indicates that the game is in one of three states:
/// [`PlacingTile`], [`ResolvingMerge`], or [`BuyingStock`].
#[derive(Debug, Clone)]
pub struct Ambiguous {
    state: AmbiguousState,
}
impl State for Ambiguous {}

#[derive(Debug, Clone)]
enum AmbiguousState {
    PlacingTile(PlacingTile),
    ResolvingMerge(ResolvingMerge),
    BuyingStock(BuyingStock),
}

impl State for GameOver {}

impl Game<Ambiguous> {

    /// Gets the action required of the active player in order to advance the
    /// game.
    #[inline]
    pub fn needed_action(&self) -> ActionRequest {
        match &self.state.state {
            AmbiguousState::PlacingTile(_) => {
                ActionRequest::PlayTile
            },
            AmbiguousState::ResolvingMerge(state) => {
                ActionRequest::ResolveMergeStock {
                    defunct: state.current_defunct(),
                    into: state.company_into()
                }
            },
            AmbiguousState::BuyingStock(_) => {
                ActionRequest::BuyStock
            },
        }
    }

    /// Determines which of the three states this game is in, and then
    /// constructs an object to represent the game in that state.
    pub fn disambiguate(self) -> GameDisambiguation {
        use GameDisambiguation::*;
        
        match self.state.state {
            AmbiguousState::PlacingTile(state) => PlacingTile(Game {
                data: self.data,
                state,
            }),
            AmbiguousState::ResolvingMerge(state) => ResolvingMerge(Game {
                data: self.data,
                state,
            }),
            AmbiguousState::BuyingStock(state) => BuyingStock(Game {
                data: self.data,
                state,
            }),
        }
    }

    /// This function takes a player action and attempts to advance the game. If
    /// successful, [`Ok`] is returned enclosing a [`Result`] that captures a
    /// potential game over. If the action is invalid, the moved `self`
    /// parameter will be returned, along with an [`InvalidMessageReason`], in
    /// an [`Err`].
    pub fn try_advance_game(self, action: &TaggedPlayerAction)
        -> TryGameUpdateResult<Ambiguous, Ambiguous>
    {
        match self.disambiguate() {
            GameDisambiguation::PlacingTile(game) => {
                match game.check_player_action(action) {
                    Ok(advance) => {
                        Ok(Ok(game.advance_game(advance).into()))
                    },
                    Err(invalid) => {
                        Err((game.into(), invalid))
                    },
                }
            },
            GameDisambiguation::ResolvingMerge(game) => {
                match game.check_player_action(action) {
                    Ok(advance) => {
                        Ok(game.step_merge(advance))
                    },
                    Err(invalid) => {
                        Err((game.into(), invalid))
                    }
                }
            },
            GameDisambiguation::BuyingStock(game) => {
                match game.check_player_action(action) {
                    Ok(advance) => {
                        Ok(game.advance_game(advance).map(|g| g.into()))
                    },
                    Err(invalid) => {
                        Err((game.into(), invalid))
                    },
                }
            },
        }
    }

    pub fn speed_play<I: IntoIterator<Item = TaggedPlayerAction>>(self, moves: I)
         -> TryGameUpdateResult<Ambiguous, Ambiguous>
    {
        let moves = moves.into_iter();

        let mut game: Game<Ambiguous> = self.into();

        for mv in moves {
            game = match game.try_advance_game(&mv)? {
                Ok(game) => game,
                Err(game_over) => return Ok(Err(game_over)),
            }
        }

        Ok(Ok(game))
    }
}

pub enum GameDisambiguation {
    PlacingTile(Game<PlacingTile>),
    ResolvingMerge(Game<ResolvingMerge>),
    BuyingStock(Game<BuyingStock>),
}

impl From<Game<PlacingTile>> for Game<Ambiguous> {
    fn from(value: Game<PlacingTile>) -> Self {
        Game {
            data: value.data,
            state: Ambiguous {
                state: AmbiguousState::PlacingTile(value.state)
            },
        }
    }
}

impl From<Game<MaybeResolvingMerge>> for Game<Ambiguous> {
    fn from(value: Game<MaybeResolvingMerge>) -> Self {
        match value.decide_merge() {
            Ok(skipper) => value.skip_merge(skipper).into(),
            Err(starter) => value.commence_merge(starter).into(),
        }
    }
}

impl From<Game<ResolvingMerge>> for Game<Ambiguous> {
    fn from(value: Game<ResolvingMerge>) -> Self {
        Game {
            data: value.data,
            state: Ambiguous {
                state: AmbiguousState::ResolvingMerge(value.state)
            },
        }
    }
}

impl From<Game<BuyingStock>> for Game<Ambiguous> {
    fn from(value: Game<BuyingStock>) -> Self {
        Game {
            data: value.data,
            state: Ambiguous {
                state: AmbiguousState::BuyingStock(value.state)
            },

        }
    }
}
