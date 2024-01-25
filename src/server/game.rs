use std::collections::HashMap;

use tokio::sync::broadcast;

use crate::game::kernel::{self, Game, GameDisambiguation, GameUpdateResult};
use crate::game::tile::{Tile, Boneyard, Hand};
use crate::game::messages::*;

use super::{PrivateBroadcast, ServerBroadcast};

/// Handles the server side of a game.
#[derive(Debug)]
pub struct ServerGame {
    broadcaster: broadcast::Sender<ServerBroadcast>,
    _impl: Option<ServerGameImpl>,
}

#[derive(Debug)]
struct ServerGameImpl {
    boneyard: Boneyard<Tile>,
    game: Game<kernel::Ambiguous>,
    player_tiles: HashMap<Box<str>, Hand>,
    start: GameStart,
    action_history: Vec<TaggedPlayerAction>,
}

impl ServerGame {
    /// Creates a new [`ServerGame`] with no game in progress.
    pub fn new(broadcaster: broadcast::Sender<ServerBroadcast>) -> Self {
        Self { broadcaster, _impl: None }
    }

    /// Makes a copy of this game's message history and returns it if there is a
    /// game in progress.
    pub fn history(&self) -> Option<GameHistory> {
        self._impl.as_ref().map(|i| GameHistory {
            start: i.start.clone(),
            actions: i.action_history.to_owned().into_boxed_slice(),
        })
    }

    /// Broadcasts and records any successful player actions. This function
    /// assumes that a game is in progress and panics otherwise.
    fn broadcast_player_action(&mut self, history: &mut Vec<TaggedPlayerAction>, action: TaggedPlayerAction) {
        history.push(action.clone());
        self.broadcaster.send(ServerBroadcast::PlayerMove { action }).unwrap();
    }

    /// Starts the game with the specified starting cash and players. This
    /// function only succeeds if no game is in progress, in which case the
    /// function will return `true`. If there is a game in progress, this
    /// function is a no-op and returns `false`. Broadcasts any messages that are
    /// needed to facilitate the game.
    pub fn start(
        &mut self,
        starting_cash: u32,
        player_names: impl IntoIterator<Item = Box<str>>,
        admin_name: Box<str>,
    ) {
        if self._impl.is_some() {
            self.broadcaster.send(ServerBroadcast::Private {
                target_player: admin_name,
                message: PrivateBroadcast::Invalid {
                    reason: InvalidMessageReason::GameAlreadyStarted
                }
            }).unwrap();
            return;
        }

        let mut boneyard = Tile::boneyard();

        // Get random starting tiles for each player.
        let mut players_and_tiles: Vec<_> = player_names.into_iter()
            .map(|name| (name, boneyard.remove().unwrap()))
            .collect();
        // Determine player start order
        players_and_tiles.sort_by(|(_, tile1), (_, tile2)| tile1.cmp(&tile2));

        // Separate the players and tiles for the game start info
        let (players, tiles): (Vec<_>, Vec<_>) = players_and_tiles.clone().into_iter().unzip();

        // Draw each player's hands
        let initial_hands: HashMap<_, _> = players_and_tiles.into_iter()
            .map(|(player, _)| {
                (
                    player,
                    Hand::from_boneyard(&mut boneyard).unwrap()
                )
            })
            .collect();
        
        // Make the HashMap to be sent
        let player_tiles: HashMap<_, _> = initial_hands.iter()
            .map(|(k, v)| (k.clone(), Hand::from(*v)))
            .collect();

        let game_start_info = GameStart {
            starting_cash, 
            play_order: players.into_boxed_slice(),
            tiles_placed: tiles.into_boxed_slice(),
        };
    
        let game = Game::start(&game_start_info);

        // Broadcast the game start message
        self.broadcaster.send(
            ServerBroadcast::GameStart {
                info: game_start_info.clone(),
                initial_hands
            }
        ).unwrap();

        // Send the first YourTurn
        self.broadcaster.send(ServerBroadcast::Private {
            target_player: game.active_player().to_owned().into_boxed_str(),
            message: PrivateBroadcast::YourTurn {
                request: ActionRequest::PlayTile
            }
        }).unwrap();

        self._impl = Some(ServerGameImpl {
            boneyard,
            game: game.into(),
            start: game_start_info,
            player_tiles,
            action_history: Vec::new(),
        });
    }

    /// Updates this game and broadcasts all the messages needed to facilitate
    /// the game.
    pub fn update(&mut self, action: TaggedPlayerAction) {
        // Take the game, sending a message if there is no game
        let mut game_impl = match self._impl.take() {
            Some(v) => v,
            None => {
                self.broadcaster.send(ServerBroadcast::Private {
                    target_player: action.player_name,
                    message: PrivateBroadcast::Invalid {
                        reason: InvalidMessageReason::NoGameStarted
                    }
                }).unwrap();
                return;
            },
        };
        let history = &mut game_impl.action_history;
        let game = game_impl.game;

        let big_result = match game.disambiguate() {
            GameDisambiguation::PlacingTile(game) => {
                match game.check_player_action(&action) {
                    Ok(advance) => {
                        // Remove the tile from the player's hand
                        let success = game_impl.player_tiles
                            .get_mut(&action.player_name).unwrap()
                            .remove_tile(advance.placement().tile);
                        
                        // Return an error if player doesn't have the tile in their hand
                        if !success {
                            Err((game.into(), InvalidMessageReason::TileNotFound))
                        }
    
                        else {

                            // Send the message
                            self.broadcast_player_action(history, action.clone());

                            // Decide whether to resolve the merge
                            let game = game.advance_game(advance);
                            match game.decide_merge() {
                                Ok(no_merge) => {
                                    Ok(Ok(game.skip_merge(no_merge).into()))
                                },
                                Err(merge) => {
        
                                    let game = game.commence_merge(merge);
        
                                    // Send the defunct company message
                                    self.broadcaster.send(ServerBroadcast::CompanyDefunct {
                                        defunct: game.current_merge().0,
                                        results: game.principle_shareholders()
                                            .to_vec()
                                            .into_boxed_slice()
                                    }).unwrap();
        
                                    Ok(Ok(game.into()))
                                },
                            }
                        }
                    },
                    Err(invalid) => {
                        Err(((game.into()), invalid))
                    },
                }
            },
            GameDisambiguation::ResolvingMerge(game) => {
                match game.check_player_action(&action) {
                    Ok(advance) => {
                        self.broadcast_player_action(history, action.clone());
    
                        match advance {
                            Ok(merge) => {
                                let mut game = game;
                                let another_defunct = game.continue_merge(merge);
    
                                if let Some(next_merge) = another_defunct {
                                    self.broadcaster.send(ServerBroadcast::CompanyDefunct {
                                        defunct: game.current_merge().0,
                                        results: game.principle_shareholders()
                                            .to_vec()
                                            .into_boxed_slice()
                                    }).unwrap();
                                };
    
                                Ok(Ok(game.into()))
                            },
                            Err(merge_done) => {
                                Ok(game.finish_merge(merge_done).map(|g| g.into()))
                            },
                        }
                    },
                    Err(invalid) => {
                        Err((game.into(), invalid))
                    }
                }
            },
            GameDisambiguation::BuyingStock(game) => {
                match game.check_player_action(&action) {
                    Ok(advance) => {
                        self.broadcast_player_action(history, action.clone());
    
                        // Draw and send the new tile
                        let new_tile = game_impl.boneyard.remove().unwrap();
                        game_impl.player_tiles.get_mut(&action.player_name).unwrap()
                            .insert_tile(new_tile)
                            .unwrap();
                        self.broadcaster.send(ServerBroadcast::Private {
                            target_player: action.player_name.clone(),
                            message: PrivateBroadcast::TileDraw { tile: new_tile }
                        }).unwrap();
    
                        Ok(game.advance_game(advance).map(|g| g.into()))
                    },
                    Err(invalid) => {
                        Err((game.into(), invalid))
                    },
                }
            },
        };

        // Propagate and send the invalid message
        let game: GameUpdateResult<kernel::Ambiguous> = match big_result {
            Ok(v) => v,
            Err((game, invalid)) => {

                // Put the game and impl back in place
                game_impl.game = game;
                self._impl = Some(game_impl);

                self.broadcaster.send(ServerBroadcast::Private {
                    target_player: action.player_name,
                    message: PrivateBroadcast::Invalid { reason: invalid }
                }).unwrap();
                return;
            },
        };

        match game {
            Ok(game) => {
                // Send the action request
                self.broadcaster.send(ServerBroadcast::Private {
                    target_player: game.active_player().to_owned().into_boxed_str(),
                    message: PrivateBroadcast::YourTurn { request: game.needed_action() }
                }).unwrap();

                // Put the game and impl back in place
                game_impl.game = game;
                self._impl = Some(game_impl);
            },
            // Handle a game over
            Err(game_over) => {

                let reason = game_over.reason().clone();
                let results = game_over.tally_results();

                // Send messages for the final companies
                results.shareholder_results.into_iter()
                    .filter_map(|(cmp, r)| r.map(|r| (cmp, r)))
                    .for_each(|(defunct, results)| {
                        self.broadcaster.send(ServerBroadcast::CompanyDefunct {
                            defunct,
                            results
                        }).unwrap();
                    });

                // Send the game over message
                self.broadcaster.send(ServerBroadcast::GameOver { 
                    reason,
                    results: results.final_standings
                }).unwrap();
            },
        };
    }
    
    /// Attempts to swap a dead tile out of the player's hand. If the player
    /// does not have the tile in question, this function will notify the player
    /// as necessary.
    pub fn swap_dead_tile(&mut self, player_name: Box<str>, tile: Tile) {
        
        let game_impl = match self._impl.as_mut() {
            Some(v) => v,
            None => {
                self.broadcaster.send(ServerBroadcast::Private {
                    target_player: player_name,
                    message: PrivateBroadcast::Invalid {
                        reason: InvalidMessageReason::NoGameStarted
                    }
                }).unwrap();
                return;
            },
        };

        // Check if the tile is dead
        if !game_impl.game.board().dead_tile(tile) {
            self.broadcaster.send(ServerBroadcast::Private {
                target_player: player_name,
                message: PrivateBroadcast::Invalid {
                    reason: InvalidMessageReason::NotDeadTile
                }
            }).unwrap();
            return;
        }

        // We can't use `get_hand_mut` because we need a disjoint borrow
        let player_hand = game_impl.player_tiles.get_mut(&player_name).unwrap();

        // First attempt remove the dead tile from the hand to see if it's present
        let success = player_hand.remove_tile(tile);

        if !success {
            self.broadcaster.send(ServerBroadcast::Private {
                target_player: player_name,
                message: PrivateBroadcast::Invalid {
                    reason: InvalidMessageReason::TileNotFound
                }
            }).unwrap();
            return;
        }

        // Then reinsert. Since we just removed a tile, this is guaranteed to succeed.
        let new_tile = game_impl.boneyard.remove().unwrap();
        player_hand.insert_tile(new_tile).unwrap();

        // Notify the players that the dead tile switch occurred
        self.broadcaster.send(ServerBroadcast::DeadTile {
            player_name,
            dead_tile: tile
        }).unwrap();
    }

    /// Forcibly ends the game. Returns `false` if there is no game to end.
    pub fn end(&mut self, admin_name: Box<str>) {
        let game_impl = match self._impl.take() {
            Some(v) => v,
            None => {
                self.broadcaster.send(ServerBroadcast::Private {
                    target_player: admin_name,
                    message: PrivateBroadcast::Invalid {
                        reason: InvalidMessageReason::NoGameStarted
                    }
                }).unwrap();
                return
            },
        };

        let game_over = game_impl.game.end_early();

        let reason = game_over.reason().clone();
        let results = game_over.tally_results();

        // Send messages for the final companies
        results.shareholder_results.into_iter()
            .filter_map(|(cmp, r)| r.map(|r| (cmp, r)))
            .for_each(|(defunct, results)| {
                self.broadcaster.send(ServerBroadcast::CompanyDefunct {
                    defunct,
                    results
                }).unwrap();
            });

        // Send the game over message
        self.broadcaster.send(ServerBroadcast::GameOver { 
            reason,
            results: results.final_standings
        }).unwrap();
    }
}

// /// Starts the game with the specified starting cash and players, and broadcasts
// /// the messages to facilitate the game.
// pub fn start(
//     boneyard: &mut Boneyard<Tile>,
//     broadcaster: &broadcast::Sender<ServerBroadcast>,
//     starting_cash: u32,
//     player_names: Vec<Box<str>>
// ) -> Game<kernel::PlacingTile> {

//     // Get random tiles for each player.
//     let mut players_and_tiles: Vec<_> = player_names.into_iter()
//         .map(|name| (name, boneyard.remove().unwrap()))
//         .collect();
//     players_and_tiles.sort_by(|(_, tile1), (_, tile2)| tile1.cmp(&tile2));
//     let (players, tiles): (Vec<_>, Vec<_>) = players_and_tiles.iter()
//         // Convert from a tuple reference to tuple of references
//         .map(|(name, tile)| (name.clone(), *tile))
//         .unzip();

//     let game_start_info = GameStart {
//         starting_cash, 
//         play_order: players.into_boxed_slice(),
//         tiles_placed: tiles.into_boxed_slice(),
//     };

//     let init_game = Game::start(&game_start_info);

//     // Determine the players' tiles
//     let hands = init_game.players().iter()
//         .map(|(player, _data)| {
//             let hand = [(); 6]
//                 .map(|_| {
//                     // The boneyard shouldn't run out of tiles before the game begins.
//                     boneyard.remove().unwrap()
//                 });

//             (player.clone(), hand)
//         })
//         .collect();

//     let game = init_game.accept_initial_hands(&hands);

//     // Create and send the GameStart message
//     let game_start_message = ServerBroadcast::GameStart {
//         info: game_start_info,
//         initial_hands: hands
//     };
//     broadcaster.send(game_start_message).unwrap();

//     game
// }

// /// Updates the game and broadcasts any necessary messages.
// pub fn update(
//     broadcaster: &broadcast::Sender<ServerBroadcast>,
//     action: TaggedPlayerAction,
//     game_and_boneyard: &mut Option<(Game<kernel::Ambiguous>, Boneyard<Tile>)>
// ) {
//     // Take the game, sending a message if there is no game
//     let (game, mut boneyard) = match game_and_boneyard.take() {
//         Some(v) => v,
//         None => {
//             broadcaster.send(ServerBroadcast::Private {
//                 target_player: action.player_name,
//                 message: PrivateBroadcast::Invalid {
//                     reason: InvalidMessageReason::NoGameStarted
//                 }
//             }).unwrap();
//             return;
//         },
//     };

//     let big_result = match game.disambiguate() {
//         GameDisambiguation::PlacingTile(mut game) => {
//             match game.check_player_action(&action) {
//                 Ok(advance) => {
//                     broadcaster.send(ServerBroadcast::PlayerMove {
//                         action: action.clone()
//                     }).unwrap();

//                     // Remove the tile from the player's hand
//                     game.remove_tile(&action.player_name, advance.placement().tile).unwrap();

//                     // Decide whether to resolve the merge
//                     let game = game.advance_game(advance);
//                     match game.decide_merge() {
//                         Ok(no_merge) => {
//                             Ok(Ok(game.skip_merge(no_merge).into()))
//                         },
//                         Err(merge) => {

//                             let game = game.commence_merge(merge);

//                             // Send the defunct company message
//                             broadcaster.send(ServerBroadcast::CompanyDefunct {
//                                 defunct: game.current_merge().0,
//                                 results: game.principle_shareholders()
//                                     .to_vec()
//                                     .into_boxed_slice()
//                             }).unwrap();

//                             Ok(Ok(game.into()))
//                         },
//                     }
//                 },
//                 Err(invalid) => {
//                     Err(((game.into()), invalid))
//                 },
//             }
//         },
//         GameDisambiguation::ResolvingMerge(game) => {
//             match game.check_player_action(&action) {
//                 Ok(advance) => {
//                     broadcaster.send(ServerBroadcast::PlayerMove { action: action.clone() }).unwrap();

//                     match advance {
//                         Ok(merge) => {
//                             let mut game = game;
//                             let another_defunct = game.continue_merge(merge);

//                             if let Some(next_merge) = another_defunct {
//                                 broadcaster.send(ServerBroadcast::CompanyDefunct {
//                                     defunct: game.current_merge().0,
//                                     results: game.principle_shareholders()
//                                         .to_vec()
//                                         .into_boxed_slice()
//                                 }).unwrap();
//                             };

//                             Ok(Ok(game.into()))
//                         },
//                         Err(merge_done) => {
//                             Ok(game.finish_merge(merge_done).map(|g| g.into()))
//                         },
//                     }
//                 },
//                 Err(invalid) => {
//                     Err((game.into(), invalid))
//                 }
//             }
//         },
//         GameDisambiguation::BuyingStock(mut game) => {
//             match game.check_player_action(&action) {
//                 Ok(advance) => {
//                     broadcaster.send(ServerBroadcast::PlayerMove {
//                         action: action.clone()
//                     }).unwrap();

//                     // Draw and send the new tile
//                     let new_tile = boneyard.remove().unwrap();
//                     game.insert_tile(&action.player_name, new_tile).unwrap();
//                     broadcaster.send(ServerBroadcast::Private {
//                         target_player: action.player_name.clone(),
//                         message: PrivateBroadcast::TileDraw { tile: new_tile }
//                     }).unwrap();

//                     Ok(game.advance_game(advance).map(|g| g.into()))
//                 },
//                 Err(invalid) => {
//                     Err((game.into(), invalid))
//                 },
//             }
//         },
//     };

//     // Propagate and send the invalid message
//     let game: Result<Game<kernel::Ambiguous>, _> = match big_result {
//         Ok(v) => v,
//         Err((game, invalid)) => {
//             *game_and_boneyard = Some((game, boneyard));
//             broadcaster.send(ServerBroadcast::Private {
//                 target_player: action.player_name,
//                 message: PrivateBroadcast::Invalid { reason: invalid }
//             }).unwrap();
//             return;
//         },
//     };

//     match game {
//         Ok(game) => {
//             // Send the action request
//             broadcaster.send(ServerBroadcast::Private {
//                 target_player: action.player_name,
//                 message: PrivateBroadcast::YourTurn { request: game.needed_action() }
//             }).unwrap();

//             // Put the new game back
//             *game_and_boneyard = Some((game, boneyard));
//         },
//         // Handle a game over
//         Err(game_over) => {

//             let reason = game_over.reason().clone();
//             let results = game_over.tally_results();

//             // Send messages for the final companies
//             results.shareholder_results.into_iter()
//                 .filter_map(|(cmp, r)| r.map(|r| (cmp, r)))
//                 .for_each(|(defunct, results)| {
//                     broadcaster.send(ServerBroadcast::CompanyDefunct {
//                         defunct,
//                         results
//                     }).unwrap();
//                 });

//             // Send the game over message
//             broadcaster.send(ServerBroadcast::GameOver { 
//                 reason,
//                 results: results.final_standings
//             }).unwrap();
//         },
//     };
// }

// //     // Move the initial game state into the future
//     // let game_over = loop {

//     //     let game_result = place_tile_phase(
//     //         turn_start_game, &broadcaster, &mut receiver
//     //     ).await;
//     //     let game = match game_result {
//     //         Ok(game) => game,
//     //         Err(game_over) => break game_over,
//     //     };

//     //     // Merging
//     //     let game_result = match game.decide_merge() {
//     //         Ok(advancer) => {
//     //             Ok(game.skip_merge(advancer))
//     //         },
//     //         Err(advancer) => {
//     //             merge_phase(
//     //                 game.commence_merge(advancer), &broadcaster, &mut receiver
//     //             ).await
//     //         },
//     //     };
//     //     let game = match game_result {
//     //         Ok(game) => game,
//     //         Err(game_over) => break game_over,
//     //     };

//     //     let game_result = buy_phase(game, &broadcaster, &mut receiver, &mut boneyard).await;
//     //     turn_start_game = match game_result {
//     //         Ok(game) => game,
//     //         Err(game_over) => break game_over,
//     //     }
//     // };

//     // // End the game
//     // let reason = game_over.reason().clone();
//     // let results = game_over.tally_results();

//     // // Send all the principle shareholder results
//     // for (company, results) in results.shareholder_results {
//     //     results.map(|results| {
//     //         broadcaster.send(ServerBroadcast::CompanyDefunct {
//     //             defunct: company,
//     //             results
//     //         }).unwrap();
//     //     });
//     // }

//     // broadcaster.send(ServerBroadcast::GameOver {
//     //     reason,
//     //     results: results.final_standings
//     // }).unwrap();
// }

// fn request_action(
//     broadcaster: &broadcast::Sender<ServerBroadcast>,
//     from_player: Box<str>,
//     request: ActionRequest,
// ) {
//     broadcaster.send(ServerBroadcast::Private {
//         target_player: from_player,
//         message: PrivateBroadcast::YourTurn { request }
//     }).unwrap();
// }

// fn send_invalid_message(
//     broadcaster: &broadcast::Sender<ServerBroadcast>,
//     name: Box<str>,
//     reason: InvalidMessageReason,
// ) {
//     broadcaster.send(ServerBroadcast::Private {
//         target_player: name,
//         message: PrivateBroadcast::Invalid { reason },
//     }).unwrap();
// }

// /// Advances the game to the next phase and sends any necessary messages.
// /// Requests the next action from the player.
// fn place_tile_phase(
//     game: Game<PlacingTile>,
//     broadcaster: &broadcast::Sender<ServerBroadcast>,
//     advancer: PlacingTileStateAdvance,
// ) -> Result<Game<Ambiguous>, Game<GameOver>> {

//     let game = game.advance_game(advancer);

//     match game.decide_merge() {
//         Ok(no_merge) => {
//             let game = game.skip_merge(no_merge);

//             // Request the next action
//             broadcaster.send(ServerBroadcast::Private {
//                 target_player: game.active_player().into(),
//                 message: PrivateBroadcast::YourTurn {
//                     request: ActionRequest::BuyStock
//                 }
//             }).unwrap();

//             Ok(game.into())
//         },
//         Err(merge) => {
//             let game = game.commence_merge(merge);
//         },
//     }

//     let player_name = action.player_name.clone();
//     match game.check_player_action(&action) {
//         Ok(advancer) => {
            
//             broadcaster.send(ServerBroadcast::PlayerMove {
//                 action: TaggedPlayerAction {
//                     player_name: player_name.clone(),
//                     action: PlayerAction::PlayTile {
//                         placement: *advancer.placement()
//                     }
//                 }
//             }).unwrap();

//             return Ok(game.advance_game(advancer));
//         },
//         Err(err) => send_invalid_message(
//             &broadcaster, player_name, err
//         ),
//     };
// }

// fn merge_phase(
//     game: Game<ResolvingMerge>,
//     broadcaster: &broadcast::Sender<ServerBroadcast>,
//     action: TaggedPlayerAction,
// ) -> Result<Game<BuyingStock>, Game<GameOver>> {

//     let mut game = game;

//     loop {

//         // Possibly send principle shareholder results
//         if game.beginning_of_new_merge() {
//             broadcaster.send(ServerBroadcast::CompanyDefunct {
//                 defunct: game.current_merge().0, 
//                 results: game.principle_shareholders()
//                     .to_vec()
//                     .into_boxed_slice()
//             }).unwrap();
//         }

//         // Request the action from the player
//         let (defunct, into) = game.current_merge();
//         request_action(&broadcaster,
//             game.active_player()    
//                 .to_string()
//                 .into_boxed_str(),
//             ActionRequest::ResolveMergeStock {
//                 defunct, into
//             }
//         );

//         let message = match receiver.recv().await {
//             Some(v) => v,
//             None => return Err(game.end_early()),
//         };

//         let advancer_result = game.check_player_action(&message);

//         let advancer = match advancer_result {
//             Ok(v) => v,
//             Err(err) => {
//                 send_invalid_message(
//                     &broadcaster, message.player_name, err
//                 );
//                 continue;
//             }
//         };

//         // Create the message
//         let action = match &advancer {
//             Ok(advancer) => PlayerAction::ResolveMergeStock {
//                 selling: advancer.selling(),
//                 trading: advancer.trading(),
//                 keeping: advancer.keeping()
//             },
//             Err(advancer) => PlayerAction::ResolveMergeStock {
//                 selling: advancer.selling(),
//                 trading: advancer.trading(),
//                 keeping: advancer.keeping()
//             },
//         };

//         // Send the action
//         broadcaster.send(ServerBroadcast::PlayerMove{
//             action: TaggedPlayerAction {
//                 player_name,
//                 action
//             }
//         }).unwrap();

//         match advancer {
//             Ok(advancer) => {
//                 game.continue_merge(advancer);
//             },
//             Err(advancer) => {
//                 return game.finish_merge(advancer);
//             },
//         };
//     }
// }

// fn buy_phase(
//     game: Game<BuyingStock>,
//     broadcaster: &broadcast::Sender<ServerBroadcast>,
//     action: TaggedPlayerAction,
//     boneyard: &mut Boneyard<Tile>,
// ) -> Result<Game<PlacingTile>, Game<GameOver>> {

//     request_action(&broadcaster,
//         game.active_player()
//             .to_string()
//             .into_boxed_str(),
//         ActionRequest::BuyStock
//     );

//     loop {
//         let message = match receiver.recv().await {
//             Some(v) => v,
//             None => return Err(game.end_early()),
//         };

//         let player_name = message.player_name.clone();
//         match game.check_player_action(&message) {
//             Ok(advancer) => {
//                 // Broadcast the move
//                 broadcaster.send(ServerBroadcast::PlayerMove {
//                     action: TaggedPlayerAction {
//                         player_name: player_name.clone(),
//                         action: PlayerAction::BuyStock {
//                             stock: advancer.stock(),
//                         }
//                     }
//                 }).unwrap();

//                 // Send the player's new tile
//                 broadcaster.send(ServerBroadcast::Private {
//                     target_player: player_name,
//                     message: PrivateBroadcast::TileDraw {
//                         tile: boneyard.remove().unwrap()
//                     }
//                 }).unwrap();

//                 return game.advance_game(advancer);
//             },
//             Err(err) => send_invalid_message(
//                 &broadcaster, player_name, err
//             ),
//         };
//     }
// }

// // /// Manages the asynchronous processes associated with the game. Owns the game
// /// board.
// struct Game {
//     /// The means by which the game will receive updates. Closing the sending
//     /// half of this channel will tell the game to cleanly end early.
//     receiver: mpsc::Receiver<PlayerAction>,
//     /// Emits messages to tell the players what to do.
//     broadcaster: broadcast::Sender<ServerBroadcast>,
//     /// Records the number of stocks that are purchased by players for each company
//     stock_bank: CompanyMap<u8>,
//     /// The tiles that have not been drawn
//     boneyard: Boneyard<Tile>,
// }

// /// Any function that could discover the game is over before returning should
// /// return this object.
// type MaybeGameOver<T> = Result<T, GameOver>;

// // Private facilitating methods
// impl Game {

//     fn broadcast(&self, message: ServerBroadcast) {
//         self.broadcaster.send(message).unwrap();
//     }

//     /// Enters a loop until a message meeting the expectations is received, or
//     /// an error occurs. If the expectation function yields [`Err`], the error
//     /// will be sent to the client and the loop will continue.
//     async fn await_action<F, R>(&mut self, mut expectations: F) -> MaybeGameOver<R>
//         where
//             F: FnMut(&Game, PlayerAction) -> Result<R, InvalidMessageReason>,
//     {
//         loop {
//             let message = self.receiver.recv().await
//                 .ok_or(GameOver::EndedEarly)?;

//             let result = expectations(self, message.clone());

//             match result {
//                 Ok(ret) => return Ok(ret),
//                 Err(reason) => {
//                     self.broadcast(
//                         ServerBroadcast::Private {
//                             player_name: message.player_name.clone(),
//                             message: PrivateSpecificBroadcast::Invalid(reason),
//                         }
//                     );
//                 },
//             }
//         }
//     }

//     /// Determines the resolve order and principle shareholder bonues, but does
//     /// not do payouts or send messages.
//     fn get_principle_bonuses(&self, defunct: Company) -> Vec<(String, PrincipleShareholderResult)> {

//         // This is why we need to be able to pass comparators to binary heaps.
//         // NewType is annoying.
//         use std::cmp::Ordering;

//         struct HeapEntry<'a>(&'a String, &'a PlayerData, u8);

//         impl<'a> PartialEq for HeapEntry<'a> {
//             fn eq(&self, other: &Self) -> bool {
//                 self.2 == other.2
//             }
//         }
//         impl<'a> Eq for HeapEntry<'a> {}

//         impl<'a> PartialOrd for HeapEntry<'a> {
//             fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
//                 self.2.partial_cmp(&other.2)
//             }
//         }

//         impl<'a> Ord for HeapEntry<'a> {
//             fn cmp(&self, other: &Self) -> Ordering {
//                 self.2.cmp(&other.2)
//             }
//         }

//         let mut heap = BinaryHeap::new();

//         for (name, data) in &self.players {
//             heap.push(HeapEntry(name, data, data.holdings[defunct]));
//         }

//         let mut i = 1;
//         let mut vec = Vec::<(String, PrincipleShareholderResult)>::new();
//         while let Some(HeapEntry(name, _data, holdings)) = heap.pop() {

//             let previous = vec.last();

//             let position = match previous {
//                 Some(prev) => {
//                     if &prev.1.shares == &holdings {
//                         prev.1.position
//                     } else {
//                         i
//                     }
//                 }
//                 None => 1,
//             };
            
//             vec.push(
//                 (
//                     name.clone(),
//                     PrincipleShareholderResult {
//                         shares: holdings,
//                         position,
//                         prize: {
//                             let multiplier = match position {
//                                 1 => 10,
//                                 2 => 5,
//                                 _ => 0,
//                             };
//                             self.board.stock_price(defunct) * multiplier
//                         },
//                     }
//                 )
//             );

//             i += 1;
//         }

//         vec
//     }

//     /// Pays out principle bonuses and retrieves the merge resolve order.
//     fn pay_principle_bonuses(&mut self, defunct: Company) -> Vec<String> {

//         self.get_principle_bonuses(defunct).into_iter()
//             .map(|(name, results)| {

//                 // Do all the messaging and prize paying
//                 let data = self.players.get_mut(&name).unwrap();
//                 data.money += results.prize;
//                 self.broadcast(
//                     ServerBroadcast::PublicSpecific {
//                         player_name: name.clone(),
//                         message: PublicSpecificBroadcast::HasStockInDefunct(results),
//                     }
//                 );

//                 // Return back the name
//                 name
//             })
//             .collect()
//     }
    
//     /// Resolves one defunct company merging into one larger company.
//     async fn resolve_a_merge(&mut self, defunct: Company, into: Company) -> MaybeGameOver<()> {

//         // Determine resolve order
//         let resolve_order = self.pay_principle_bonuses(defunct);

//         for next_to_resolve in resolve_order.iter() {

//             self.broadcast(
//                 ServerBroadcast::Private {
//                     player_name: next_to_resolve.clone(),
//                     message: PrivateSpecificBroadcast::YourTurn(
//                         ActionRequest::ResolveMergeStock { defunct, into }
//                     )
//                 }
//             );
                
//             let (sender, selling, trading, keeping)
//                 = self.await_action(|game, action| {
                
//                 if let PlayerActionKind::ResolveMergeStock {
//                     selling,
//                     trading,
//                     keeping
//                 } = action.kind {

//                     // Ensure the player sending the message is the one who
//                     // should be sending the message
//                     if &action.player_name != next_to_resolve {
//                         return Err(InvalidMessageReason::OutOfTurn)
//                     }

//                     let player_obj = game.players.get(next_to_resolve).unwrap();

//                     // Ensure the player has that amount of stock
//                     if selling + trading + keeping != player_obj.holdings[defunct] {
//                         return Err(InvalidMessageReason::ResolvesNonexistentStock)
//                     }

//                     // Ensure the number of things being traded is even
//                     if keeping % 2 == 1 {
//                         return Err(InvalidMessageReason::TradesInOddStock)
//                     }

//                     // Ensure there's enough stock in the new company to trade for
//                     if keeping % 2 + game.stock_bank[into] >= 25 {
//                         return Err(InvalidMessageReason::OutOfStock)
//                     }

//                     Ok((next_to_resolve, selling, trading, keeping))

//                 } else {
//                     return Err(InvalidMessageReason::OutOfTurn);
//                 }
//             }).await?;

//             // Resolve the merge for this player
//             let stock_price = self.board.stock_price(defunct);
//             let player_obj = self.players.get_mut(sender).unwrap();
//             player_obj.holdings[defunct] = keeping;
//             player_obj.money += stock_price * selling as u32;
//             player_obj.holdings[into] += trading / 2;
//         }

//         Ok(())
//     }
    
//     async fn resolve_merge(&mut self, merge: &Merge) -> MaybeGameOver<()> {
//         for defunct in &merge.defunct {
//             self.resolve_a_merge(*defunct, merge.into).await?;
//         }

//         Ok(())
//     }
    
//     /// Requires a player to take their turn.
//     async fn one_turn(&mut self, player: &String) -> MaybeGameOver<()> {

//         self.broadcast(ServerBroadcast::Private {
//             player_name: player.clone(),
//             message: PrivateSpecificBroadcast::YourTurn(ActionRequest::PlayTile)
//         });

//         // First, get the tile placement
//         let (tile_position, implication)
//             = self.await_action(|game, action| {

//             if let PlayerActionKind::PlayTile(tile, implication) = action.kind {

//                 if player != &action.player_name {
//                     return Err(InvalidMessageReason::OutOfTurn);
//                 }

//                 let player_obj = game.players.get(&action.player_name).unwrap();
                
//                 // Find the tile in their hand
//                 let position = player_obj.tiles.iter().position(|&t| t == Some(tile))
//                     .ok_or(InvalidMessageReason::TileNotFound)?;

//                 game.board.check_implication(tile, &implication)?;

//                 Ok((position, implication))

//             } else {
//                 Err(InvalidMessageReason::OutOfTurn)
//             }

//         }).await?;

//         // Remove the tile from the hand
//         let tiles = &mut self.players.get_mut(player).unwrap().tiles;
//         let tile = std::mem::replace(&mut tiles[tile_position], None).unwrap();

//         // Perform extra behavior based on the implication
//         self.board.place_tile(tile, &implication);
//         match implication {
//             TilePlacementImplication::None => {}
//             TilePlacementImplication::FoundsCompany(company) => {

//                 // Give the player their free stock (if there's one to give).
//                 if self.stock_bank[company] < 25 {
//                     self.players.get_mut(player).unwrap().holdings[company] += 1;
//                     self.stock_bank[company] += 1;
//                 }
//             },
//             TilePlacementImplication::MergesCompanies(ref merge) => {
//                 self.resolve_merge(merge).await?;
//                 self.board.resolve_merge(merge);
//             },
//         };

//         self.broadcast(ServerBroadcast::PublicSpecific {
//             player_name: player.clone(),
//             message: PublicSpecificBroadcast::PlayerMove(
//                 PlayerActionKind::PlayTile(tile, implication)
//             )
//         });

//         // Then, get the stock purchases
//         self.broadcast(ServerBroadcast::Private {
//             player_name: player.clone(),
//             message: PrivateSpecificBroadcast::YourTurn(ActionRequest::BuyStock)
//         });

//         let (stock, total_cost) = 
//             self.await_action(|game, action| {

//             if player != &action.player_name {
//                 return Err(InvalidMessageReason::OutOfTurn);
//             }

//             if let PlayerActionKind::BuyStock(stock) = action.kind {

//                 // Check if the player can afford it
//                 let mut total_cost: u32 = 0;
                    
//                 for stock in stock.iter() {
//                     if let Some(company) = stock {

//                         // Check if the company exists
//                         if !game.board.company_exists(*company) {
//                             return Err(InvalidMessageReason::NonexistentCompany(*company))
//                         }

//                         // Check if there's stock to buy
//                         if game.stock_bank[*company] == 25 {
//                             return Err(InvalidMessageReason::OutOfStock);
//                         }

//                         total_cost += game.board.stock_price(*company);
//                     }
//                 }

//                 let player_obj = game.players.get(&action.player_name).unwrap();

//                 // Check if there are sufficient funds
//                 if total_cost > player_obj.money {
//                     return Err(InvalidMessageReason::InsufficientFunds {
//                         deficit: total_cost - player_obj.money
//                     });
//                 }

//                 Ok((stock, total_cost))
//             } else {
//                 Err(InvalidMessageReason::OutOfTurn)
//             }

//         }).await?;

//         // Buy the stock
//         let data = self.players.get_mut(player).unwrap();
//         for share in stock {
//             if let Some(share) = share {
//                 data.holdings[share] += 1;
//             }
//         }
//         data.money -= total_cost;

//         // Redraw a tile
//         let tile = self.boneyard.remove();

//         if let Some(tile) = tile {

//             // Replace the empty slot
//             data.tiles[tile_position] = Some(tile);

//             // Send the player their tile
//             self.broadcast(
//                 ServerBroadcast::Private {
//                     player_name: player.clone(),
//                     message: PrivateSpecificBroadcast::DrawTile(tile)
//                 }
//             );
//         }

//         Ok(())
//     }

//     /// Checks if the game is over.
//     fn is_game_over(&self) -> Option<GameOver> {
        
//         // Any dominating companies?
//         let possible_dominator = self.board.company_sizes.iter()
//             .find(|(_, size)| **size >= 41);

//         if let Some(dominator) = possible_dominator {
//             return Some(GameOver::DominatingCompany(dominator.0));
//         }

//         let out_of_stock = self.stock_bank.iter()
//             .any(|(company, stock_count)| {
//                 !(0..=1).contains(&self.board.company_sizes[company])
//                     && *stock_count >= 25
//             });
        
//         if out_of_stock {
//             return Some(GameOver::NoStock)
//         }

//         None
//     }

//     /// Ends and drops the game.
//     fn game_over(mut self, reason: GameOver) {

//         // Pay out all of the principle bonuses in every remaining company.
//         let mut existing_companies = Vec::new();
//         for (company, _size) in self.board.company_sizes.iter() {
//             if !self.board.company_exists(company) { continue; }
//             existing_companies.push(company);
//         }
//         for company in existing_companies {
//             self.pay_principle_bonuses(company);
//         }

//         // Tally up the results
//         let mut results: Vec<_> = self.players.iter()
//             .map(|(name, data)| {
//                 (name.clone(), data.money)
//             })
//             .collect();

//         results.sort_by(|(_, money1), (_, money2)| money1.cmp(&money2));

//         self.broadcast(ServerBroadcast::General(
//             GeneralServerBroadcast::GameOver { reason, results }
//         ));
//     }
// // }
