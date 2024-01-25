use crate::game::{messages::*, Company};

use super::{State, Game, place_tile::PlacingTile, ID_CHECK_FAIL};

/// The end state of a [`Game`] turn in which it is waiting for a player to
/// buy stock.
#[derive(Debug, Clone, Copy)]
pub struct BuyingStock;
impl State for BuyingStock {}

/// Advances the game state from [`BuyingStock`] to the next player's
/// [`PlacingTile`] phase.
#[derive(Debug, PartialEq, Eq)]
pub struct BuyingStockStateAdvance {
    stock: [Option<Company>; 3],
    total_cost: u32,
    game_id: usize,
}

impl BuyingStockStateAdvance {
    pub fn stock(&self) -> [Option<Company>; 3] {
        self.stock
    }

    pub fn total_cost(&self) -> u32 {
        self.total_cost
    }
}

impl Game<BuyingStock> {

    /// Takes a [`PlayerAction`], and check if it was the requested action from
    /// the active player. If so, a [`BuyingStockStateAdvance`] can be used to
    /// advance this game. This function ignores the `new_tile` field within the
    /// action. It's expected that that field, if present, is provided when
    /// advancing the state.
    pub fn check_player_action(&self, action: &TaggedPlayerAction)
        -> Result<BuyingStockStateAdvance, InvalidMessageReason>
    {
        if &self.data.player != &action.player_name {
            return Err(InvalidMessageReason::OutOfTurn);
        }

        if let PlayerAction::BuyStock{ stock } = action.action {
            Ok(self.check_buy_stock(stock)?)
        } else {
            Err(InvalidMessageReason::OutOfTurn)
        }
    }

    /// Tries to buy stock as the active player and checks if the purchase is
    /// valid. If so, a [`BuyingStockStateAdvance`] is returned that can be used
    /// to advance this game. The advancer will not provide a new tile to the
    /// player.
    pub fn check_buy_stock(&self, stock: [Option<Company>; 3])
        -> Result<BuyingStockStateAdvance, BuyStockError>
    {
        // Check if the player can afford it
        let mut total_cost: u32 = 0;
                
        for stock in stock.iter() {
            if let Some(company) = stock {

                // Check if the company exists
                if !self.board().company_exists(*company) {
                    return Err(BuyStockError::NonexistentCompany {
                        company: *company
                    })
                }

                // Check if there's stock to buy
                if self.stock_bank()[*company] == 25 {
                    return Err(BuyStockError::OutOfStock);
                }

                total_cost += self.board().stock_price(*company);
            }
        }

        let player_obj = self.players().get(&self.data.player).unwrap();

        // Check if there are sufficient funds
        if total_cost > player_obj.money {
            return Err(BuyStockError::InsufficientFunds {
                deficit: total_cost - player_obj.money
            });
        }

        Ok(BuyingStockStateAdvance {
            game_id: self.data.id,
            stock,
            total_cost
        })
    }

    /// Advances the game forward. This concludes a turn, and the game will
    /// return to the state of [`PlacingTile`], but now for the next player. The
    /// `new_tile` argument conveys one of three things.
    /// 1. If the hand of the active player is unknown, a value of [`None`] is
    ///    the only correct option. A value of [`Some`] will cause a panic.
    /// 2. For an active player with a known hand, a [`Some`] value replenishes
    ///    player's hand with a new tile.
    /// 3. If the hand of the active player is known, a [`None`] value indicates
    ///    that the boneyard used to draw tiles is empty.
    /// 
    /// # Panics
    /// 
    /// * If the hand of the active player is unknown, and `new_tile` is passed
    ///   a [`Some`] value, this function will panic.
    /// * This function panics if the `advancer` was not produced by this object.
    pub fn advance_game(
        self,
        advancer: BuyingStockStateAdvance,
    )  -> Result<Game<PlacingTile>, Game<GameOver>> {
        assert_eq!(advancer.game_id, self.data.id, "{ID_CHECK_FAIL}");

        let mut data = self.data;

        // Buy the stock
        let player_data = data.kernel.players.get_mut(&data.player).unwrap();
        for share in advancer.stock {
            if let Some(share) = share {
                player_data.holdings[share] += 1;
            }
        }
        player_data.money -= advancer.total_cost;

        // See if one company dominates
        let possible_dominator = data.kernel.board.company_sizes.iter()
            .find(|(_, size)| **size >= 41);

        if let Some(dominator) = possible_dominator {
            let state = GameOver::DominatingCompany { company: dominator.0 };
            return Err(Game { data, state });
        }

        // Check if there is any stock left to buy
        let out_of_stock = data.kernel.stock_bank.iter()
            .any(|(company, stock_count)| {
                data.kernel.board.company_exists(company) && *stock_count >= 25
            });
        
        if out_of_stock {
            let state = GameOver::NoStock;
            return Err(Game { data, state });
        }

        // Advance the game to the next player
        data.player = data.kernel.players[&data.player].next_player.clone();

        Ok(Game { data, state: PlacingTile })
    }
}
