use crate::game::messages::*;

use super::{State, Game, MaybeResolvingMerge, ID_CHECK_FAIL};

/// The beginning state of a [`Game`] turn in which it is waiting for a player to
/// place a tile.
#[derive(Debug, Clone, Copy)]
pub struct PlacingTile;
impl State for PlacingTile {}

#[derive(Debug, PartialEq, Eq)]
pub struct PlacingTileStateAdvance {
    game_id: usize,
    placement: TilePlacement,
}

impl PlacingTileStateAdvance {
    pub fn placement(&self) -> &TilePlacement {
        &self.placement
    }
}

impl Game<PlacingTile> {

    /// Takes a [`PlayerAction`], and check if it was the requested action from
    /// the active player. If so, a [`PlacingTileStateAdvance`] can be used to
    /// advance this game.
    /// 
    /// # Panics
    /// 
    /// Calling this function assumes that the player whose turn it is has a
    /// complete hand. If that is not the case, this function panics.
    pub fn check_player_action(&self, action: &TaggedPlayerAction)
        -> Result<PlacingTileStateAdvance, InvalidMessageReason>
    {
        if let PlayerAction::PlayTile { placement } = action.action {

            if self.active_player() != &*action.player_name {
                return Err(InvalidMessageReason::OutOfTurn);
            }

            self.board().check_implication(placement)?;

            Ok(PlacingTileStateAdvance {
                game_id: self.data.id,
                placement,
            })

        } else {
            Err(InvalidMessageReason::OutOfTurn)
        }
    }
    
    /// Places a tile as the active player and checks if the tile with its
    /// implication is valid. If the move was valid, a
    /// [`PlacingTileStateAdvance`] is returned that can be used to advance this
    /// game.
    pub fn check_tile(&self, placement: TilePlacement)
        -> Result<PlacingTileStateAdvance, IncorrectImplication>
    {
        self.board().check_implication(placement)?;
        Ok(PlacingTileStateAdvance {
            game_id: self.data.id,
            placement,
        })
    }
    
    /// Advances the game to the next state, which is [`MaybeResolvingMerge`].
    /// 
    /// # Panics
    /// 
    /// This function panics if the `advancer` was not produced by this object.
    pub fn advance_game(self, advancer: PlacingTileStateAdvance)
        -> Game<MaybeResolvingMerge>
    {
        assert_eq!(advancer.game_id, self.data.id, "{ID_CHECK_FAIL}");

        let mut data = self.data;

        data.kernel.board.place_tile(*advancer.placement());

        // Perform extra behavior based on the implication
        let state = match advancer.placement().implication {
            None => MaybeResolvingMerge { merge: None },
            Some(TilePlacementImplication::FoundsCompany(company)) => {

                // Give the player their free stock (if there's one to give).
                if data.kernel.stock_bank[company] < 25 {
                    data.kernel.players.get_mut(&data.player).unwrap().holdings[company] += 1;
                    data.kernel.stock_bank[company] += 1;
                }

                MaybeResolvingMerge { merge: None }
            },
            // Compute and pay out principle shareholder bonuses
            Some(TilePlacementImplication::MergesCompanies(merge)) => {
                MaybeResolvingMerge { merge: Some(merge) }
            },
        };

        Game { data, state }
    }
}