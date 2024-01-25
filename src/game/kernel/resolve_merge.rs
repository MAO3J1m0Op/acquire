use crate::game::{messages::*, Company};

use super::{Game, State, buy_stock::BuyingStock, ID_CHECK_FAIL, ambiguous::Ambiguous};

/// Intermediate state in which the [`Game`] is deciding whether to enter
/// [`ResolvingMerge`] or [`BuyingStock`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaybeResolvingMerge {
    pub(super) merge: Option<Merge>,
}
impl State for MaybeResolvingMerge {}

/// The possible intermediate state of a [`Game`] turn in which it is
/// facilitating the merge of one or more defunct companies into one large
/// company.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvingMerge {
    /// The merge that initiated this entire process. This does not change until
    /// the object is dropped.
    merge: Merge,
    /// The (possibly compound) merge being resolved.
    current_merge: Merge,
    /// The defunct that is up first to be resolved.
    current_defunct: Company,
    /// The shareholder results for the merge currently being resolved.
    shareholder_results: Vec<PrincipleShareholderResult>,
    /// The player from which action is expected.
    resolving_player: usize,
}
impl State for ResolvingMerge {}

impl ResolvingMerge {
    /// Gets the company whose defunct stock is currently being resolved.
    pub fn current_defunct(&self) -> Company {
        self.current_defunct
    }

    /// Gets the company who will prevail at the end of this merge.
    pub fn company_into(&self) -> Company {
        self.merge.into
    }
}

mod sealed {
    use super::{ContinueMerging, DoneMerging};

    pub trait SealedResolution {
        fn game_id(&self) -> usize;
    }
    impl SealedResolution for ContinueMerging {
        fn game_id(&self) -> usize {
            self.game_id
        }
    }
    impl SealedResolution for DoneMerging {
        fn game_id(&self) -> usize {
            self.game_id
        }
    }
}

/// Indicates what should be done after stepping forward a [`Game`] in the
/// [`ResolvingMerge`] state.
pub trait MergeResolution: sealed::SealedResolution {}

/// Indicates that a merge within a [`Game`] in the [`ResolvingMerge`] state is
/// not finished and pushes along the merge.
#[derive(Debug, PartialEq, Eq)]
pub struct ContinueMerging {
    /// The ID of the game that created this object.
    game_id: usize,
}
impl MergeResolution for ContinueMerging {}

/// Indicates that a merge within a [`Game`] in the [`ResolvingMerge`] state is
/// finished and concludes the merge.
#[derive(Debug, PartialEq, Eq)]
pub struct DoneMerging {
    /// The ID of the game that created this object.
    game_id: usize,
}
impl MergeResolution for DoneMerging {}

/// Passed to a [`super::Game`] in the [`ResolvingMerge`] state to completes a
/// step of the merge. Additionally, it decides whether to continue merging or
/// to be finished.
#[derive(Debug, PartialEq, Eq)]
pub struct MergeStep<N: MergeResolution + Eq> {
    /// Stock being sold by the player.
    selling: u8,
    /// Stock being kept by the player.
    keeping: u8,
    /// Stock being traded by the player.
    trading: u8,
    /// Whether this function resolves.
    resolve: N,
}

impl<N: MergeResolution + Eq> MergeStep<N> {

    pub fn selling(&self) -> u8 {
        self.selling
    }

    pub fn trading(&self) -> u8 {
        self.trading
    }

    pub fn keeping(&self) -> u8 {
        self.keeping
    }
}

impl Game<MaybeResolvingMerge> {
    /// Decide whether or not this Game has a merge to resolve. Returns the
    /// object that advances the state either way.
    pub fn decide_merge(&self)
        -> Result<DoneMerging, ContinueMerging>
    {
        match self.state.merge.as_ref() {
            Some(_) => Err(ContinueMerging {
                game_id: self.data.id,
            }),
            None => Ok(DoneMerging {
                game_id: self.data.id
            }),
        }
    }

    /// Skips the merge resolution phase of this turn, as it has been deemed
    /// unnecessary. This advances the game to the [`BuyingStock`] state.
    /// 
    /// # Panics
    /// 
    /// This function panics if the `skipper` was not produced by this object.
    pub fn skip_merge(self, skipper: DoneMerging)
        -> Game<BuyingStock>
    {
        assert_eq!(skipper.game_id, self.data.id, "{ID_CHECK_FAIL}");

        Game {
            data: self.data,
            state: BuyingStock,
        }
    }

    /// Begins resolving a merge, advancing this game to the [`ResolvingMerge`]
    /// state.
    /// 
    /// # Panics
    /// 
    /// This function panics if the `starter` was not produced by this object.
    pub fn commence_merge(mut self, starter: ContinueMerging)
        -> Game<ResolvingMerge>
    {
        assert_eq!(starter.game_id, self.data.id, "{ID_CHECK_FAIL}");

        let mut merge = self.state.merge.unwrap();
        let current_defunct = merge.pop_defunct().expect("merge with empty defunct");
        let state = ResolvingMerge {
            current_merge: merge,
            merge,
            current_defunct,
            shareholder_results: self.data.kernel.pay_principle_bonuses(current_defunct),
            resolving_player: 0,
        };

        Game {
            data: self.data,
            state,
        }
    }
}

impl Game<ResolvingMerge> {

    /// Gets the merge currently being resolved. the `.0` element is the defunct
    /// company, and the `.1` element is the company into which the defunct is
    /// merging.
    pub fn current_merge(&self) -> (Company, Company) {
        (self.state.current_defunct, self.state.current_merge.into)
    }

    /// Determines if the game is beginning a new merge in the current state.
    pub fn beginning_of_new_merge(&self) -> bool {
        self.state.resolving_player == 0
    }

    /// Gets the list of principle shareholders for the current merge.
    pub fn principle_shareholders(&self) -> &[PrincipleShareholderResult] {
        &self.state.shareholder_results[..]
    }
    
    /// Takes a [`PlayerAction`], and check if it was the requested action from
    /// the active player. If so, a [`MergeStep`] can be used to advance this
    /// game.
    pub fn check_player_action(&self, action: &TaggedPlayerAction)
    -> Result<Result<
            MergeStep<ContinueMerging>,
            MergeStep<DoneMerging>
        >, InvalidMessageReason>
    {
        let resolving_player = &self.state
            .shareholder_results[self.state.resolving_player].player;

        if let PlayerAction::ResolveMergeStock {
            selling,
            trading,
            keeping
        } = action.action {

            // Ensure the player sending the message is the one who
            // should be sending the message
            if &action.player_name != resolving_player {
                return Err(InvalidMessageReason::OutOfTurn)
            }

            Ok(self.check_merge_resolution(selling, keeping, trading)?)

        } else {
            return Err(InvalidMessageReason::OutOfTurn);
        }
    }

    /// Tries to resolve defunct stock as the active player and checks if the
    /// resolution is its implication is valid. If the move was valid, a
    /// [`MergeStep`] is returned that can be used to advance this game.
    pub fn check_merge_resolution(&self, selling: u8, keeping: u8, trading: u8)
        -> Result<Result<
                MergeStep<ContinueMerging>,
                MergeStep<DoneMerging>
            >, MergeResolveError>
    {
        let resolving_player = &self.state
            .shareholder_results[self.state.resolving_player].player;
                
        let player_obj = self.players().get(resolving_player).unwrap();

        // Ensure the player has that amount of stock
        if selling + trading + keeping != player_obj.holdings[self.state.current_defunct] {
            return Err(MergeResolveError::ResolvesNonexistentStock);
        }

        // Ensure the number of things being traded is even
        if keeping % 2 == 1 {
            return Err(MergeResolveError::TradesInOddStock);
        }

        // Ensure there's enough stock in the new company to trade for
        if keeping % 2 + self.stock_bank()[self.state.current_merge.into] >= 25 {
            return Err(MergeResolveError::OutOfStock)
        }

        // Are we done with this defunct company?
        if self.state.resolving_player == self.state.shareholder_results.len() {

            // Are we done with the merge?
            if self.state.current_merge.defunct_is_empty() {
                return Ok(Err(MergeStep {
                    selling, keeping, trading,
                    resolve: DoneMerging {
                        game_id: self.data.id,
                    }
                }))
            };
        }

        Ok(Ok(MergeStep {
            selling, keeping, trading,
            resolve: ContinueMerging {
                game_id: self.data.id,
            }
        }))
    }

    /// Updates the kernel data to resolve the defunct stock of one player.
    fn apply_merge<N: MergeResolution + Eq>(&mut self, next_step: MergeStep<N>) {
        assert_eq!(next_step.resolve.game_id(), self.data.id, "{ID_CHECK_FAIL}");

        let stock_price = self.board().stock_price(self.state.current_defunct);
        let player_obj = self.data.kernel.players.get_mut(&self.data.player).unwrap();
        player_obj.holdings[self.state.current_defunct] += next_step.keeping();
        player_obj.money += stock_price * next_step.selling() as u32;
        player_obj.holdings[self.state.current_merge.into] += next_step.trading() / 2;
    }

    /// Advances the merge along. If the merge resolution has to resolve the
    /// stock of an additional defunct company, a reference to the shareholder
    /// results will be returned.
    /// 
    /// # Panics
    /// 
    /// This function panics if the `step` was not produced by this object.
    pub fn continue_merge(&mut self, step: MergeStep<ContinueMerging>) 
        -> Option<&[PrincipleShareholderResult]>
    {
        self.apply_merge(step);
        
        self.state.resolving_player += 1;
        
        // Move to the next defunct company
        if self.state.resolving_player == self.state.shareholder_results.len() {
            let defunct = self.state.current_merge.pop_defunct().unwrap();
            self.state.shareholder_results = self.data.kernel
                .pay_principle_bonuses(defunct);
            self.state.current_defunct = defunct;
            self.state.resolving_player = 0;

            // Return the new shareholder results
            Some(&self.state.shareholder_results)

        } else { None }
    }

    /// Finishes the merge and advances the game into the [`BuyingStock`] state.
    /// 
    /// # Panics
    /// 
    /// This function panics if the `step` was not produced by this object.
    pub fn finish_merge(self, step: MergeStep<DoneMerging>)
        -> Result<Game<BuyingStock>, Game<GameOver>>
    {
        let mut game = self;
        game.apply_merge(step);

        // Update the board
        game.data.kernel.board.resolve_merge(&game.state.merge);

        // Check if there is any stock left to buy
        let out_of_stock = game.stock_bank().iter()
            .any(|(company, stock_count)| {
                game.data.kernel.board.company_exists(company) && *stock_count >= 25
            });
        
        if out_of_stock {
            let state = GameOver::NoStock;
            return Err(Game { 
                data: game.data,
                state,
            });
        }

        Ok(Game {
            data: game.data,
            state: BuyingStock,
        })
    }

    /// Steps the merge forward, handling both the cases where the merge
    /// concludes and continues. The cost is that the game returned does not
    /// have its state known at compile-time.
    pub fn step_merge(self,
        step: Result<MergeStep<ContinueMerging>, MergeStep<DoneMerging>>
    ) -> Result<Game<Ambiguous>, Game<GameOver>> {
        match step {
            Ok(not_done) => {
                let mut game = self;
                game.continue_merge(not_done);
                Ok(game.into())
            },
            Err(done) => {
                self.finish_merge(done).map(|game| game.into())
            }
        }
    }
}
