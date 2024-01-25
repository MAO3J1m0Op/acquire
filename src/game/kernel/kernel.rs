use std::collections::HashMap;

use crate::game::{CompanyMap, Company, messages::*};
use crate::game::board::Board;

use super::PlayerData;

/// Holds the core components of a game. This is moved around.
#[derive(Debug, Clone)]
pub struct GameKernel {
    /// The board of the game.
    pub board: Board,
    /// Records the number of stocks that are purchased by players for each company
    pub stock_bank: CompanyMap<u8>,
    /// All players and their data.
    pub players: HashMap<Box<str>, PlayerData>,
}

impl GameKernel {
    
    /// Computes and pays the principle bonuses for the company in the defunct
    /// slot, and stores these bonuses into the state.
    pub fn pay_principle_bonuses(&mut self, defunct: Company)
        -> Vec<PrincipleShareholderResult>
    {
        // Order the players by who has the most stock, with the principle
        // shareholder being first.
        let mut players: Vec<_> = self.players.iter().collect();
        players.sort_by(|&(_name_a, data_a), &(_name_b, data_b)| {
            data_b.holdings[defunct]
                .cmp(&data_a.holdings[defunct])
        });

        let mut vec: Vec<PrincipleShareholderResult> = vec![];
        let mut i = 1;

        for (name, data) in players {
            let position = match vec.last() {
                Some(prev) => {
                    if prev.shares == data.holdings[defunct] {
                        prev.position
                    } else {
                        i
                    }
                }
                None => 1,
            };

            let result = PrincipleShareholderResult {
                player: name.clone(),
                shares: data.holdings[defunct],
                position,
                prize: {
                    let multiplier = match position {
                        1 => 10,
                        2 => 5,
                        _ => 0,
                    };
                    self.board.stock_price(defunct) * multiplier
                },
            };
            vec.push(result);

            i += 1;
        }

        // Pay out the prize
        vec.into_iter()
            .map(|result| {
                let data = self.players.get_mut(&result.player).unwrap();
                data.money += result.prize;
                result
            })
            .collect()
    }

    pub fn get_standings(&self) -> Vec<FinalResult> {
        use std::cmp::Ordering;

        let mut final_standings: Vec<_> = self.players.iter()
            .map(|(name, data)| {
                FinalResult {
                    player_name: name.clone(),
                    final_money: data.money,
                    // Placeholder value
                    place: 0,
                }
            })
            .collect();

        final_standings.sort();

        // Compute places
        let mut prev_place = 1;
        let mut iter = final_standings.iter_mut();
        let mut prev_money = iter.next().unwrap().final_money;

        for standing in iter {
            match standing.final_money.cmp(&prev_money) {
                Ordering::Greater => unreachable!(),
                Ordering::Equal => {
                    standing.place = prev_place;
                },
                Ordering::Less => {
                    prev_place += 1;
                    standing.place = prev_place;
                },
            }
            prev_money = standing.final_money;
        }

        final_standings
    }
}
