use crate::game::{messages::*, board::Board};

use super::{Game, GameImpl, GameKernel, PlayerData, place_tile::PlacingTile};

impl Game<PlacingTile> {
    /// Begins a new game. It is expected that the order of play has already
    /// been determined through random drawing of tiles. The tiles that are
    /// drawn are accepted and placed on the board.
    /// 
    /// # Panics
    /// 
    /// It is assumed that the boneyard has enough tiles to provide for all of
    /// the players, and the function panics otherwise. The hard minimum number
    /// of players is 1, and the hard maximum number of players is 15. If this
    /// is not met, the function panics. 
    pub fn start(game_start_info: &GameStart) -> Self {
        assert!(!game_start_info.play_order.is_empty(),
            "game started with no players");
        assert!(game_start_info.play_order.len() < 15,
            "game has too many players, max is 15, got {}",
                game_start_info.play_order.len());

        let mut board = Board::new();

        // Offset the play order by one, moving the first index to the back.
        // This is to initialize the `next_player` value.
        let mut offset_play_order = game_start_info.play_order.iter();
        let first = offset_play_order.next().unwrap();
        let offset_play_order = offset_play_order.chain(std::iter::once(first));
        
        // Place the tiles on the board
        for &tile in game_start_info.tiles_placed.iter() {
            board.place_tile(TilePlacement {
                tile, implication: None
            });
        }

        let players = game_start_info.play_order.iter()
            .zip(offset_play_order)
            .enumerate()
            .map(|(order, (name, next_name))| {

                // Construct the PlayerData instances
                let data = PlayerData {
                    money: game_start_info.starting_cash,
                    holdings: Default::default(),
                    order,
                    next_player: next_name.clone(),
                };

                let name = name.to_string().into_boxed_str();

                (name, data)
            })
            .collect();

        dbg!(Self {
            data: Box::new(GameImpl::new(
                GameKernel {
                    board,
                    stock_bank: Default::default(),
                    players
                }, 
                first.clone())
            ),
            state: PlacingTile,
        })
    }
}
