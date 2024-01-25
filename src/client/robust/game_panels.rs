use crate::client::ClientGame;
use crate::game::kernel::{Game, GameDisambiguation};
use crate::game::{messages::*, CompanyMap};
use crate::game::tile::{Tile, FullHand};
use crate::server::ConnectionManager;

use self::action_panel::{ActionPanel, ActionPanelRequest};
use self::board_panel::BoardPanel;

use super::terminal::TermPanel;

mod action_panel;
mod board_panel;

/// Keeps track of the state of the game and the players who are connected.
pub struct GamePanels<'c> {
    action_panel: ActionPanel,
    board_panel: BoardPanel<'c>,
}

impl<'c> GamePanels<'c> {
    
    /// Creates a new [`GamePanels`] of size zero. It must be resized later.
    pub fn new(
        game: ClientGame,
        connections: &'c mut ConnectionManager,
    ) -> Self {
        Self {
            action_panel: ActionPanel::new(),
            board_panel: BoardPanel::new(game, connections),
        }
    }

    /// Gets a reference to the underlying [`ClientGame`] in progress.
    pub fn game(&self) -> &ClientGame {
        &self.board_panel.game
    }

    /// Starts a new game.
    pub fn start_game(&mut self,
        info: &GameStart,
        player_tiles: Option<FullHand>,
    ) {
        let game = Game::start(info).into();
        self.board_panel.game.start(game, player_tiles.map(|h| h.into()));
        self.board_panel.render();
    }

    /// Forcibly ends this game and updates the panel correspondingly. If there
    /// was no game in progress, this function does nothing and returns `false`.
    pub fn end_game(&mut self) {
        self.board_panel.game.end();
        self.cancel_action();
        self.board_panel.render();
    }

    /// Accepts a player action and re-renders the board panel.
    /// 
    /// # Panics
    /// 
    /// If there is no game in progress, this function panics.
    pub fn update_game(&mut self,
        action: &TaggedPlayerAction
    ){
        self.board_panel.game.update(action);
        self.board_panel.render();
    }

    /// Requests an action from the player.
    pub fn request_action(&mut self, request: ActionRequest) {
        let request = match request {
            ActionRequest::PlayTile => ActionPanelRequest::PlaceTile,
            ActionRequest::BuyStock => {
                let game = self.board_panel.game.game().unwrap();
                let available_companies = CompanyMap::new(&())
                    .map(|company, _| game.board().company_exists(company));
                ActionPanelRequest::BuyStock {
                    available_companies,
                }
            },
            ActionRequest::ResolveMergeStock { defunct, into } => todo!(),
        };

        self.action_panel.request_action(request, self.board_panel.game.hand())
    }

    /// Cancels any action that may be in progress.
    pub fn cancel_action(&mut self) {
        self.action_panel.cancel_action(self.board_panel.game.hand());
    }

    /// Adds a drawn tile to the player's hand. If there is no game in progress,
    /// this function panics.
    pub fn draw_tile(&mut self, new_tile: Tile) {
        let hand = self.board_panel.game.hand_mut()
            .expect("Called draw_tile on a ClientGame not faciliating a game");
        hand.insert_tile(new_tile).unwrap();
        self.render();
    }

    pub fn connections(&self) -> &ConnectionManager {
        &self.board_panel.connections
    }

    /// Executes an operation with a mutable reference to the underlying
    /// [`ConnectionManager`]. Afterward, the panel is re-rendered.
    pub fn connections_mut<F>(&mut self, op: F)
        where F: FnOnce(&mut ConnectionManager)
    {
        op(&mut self.board_panel.connections);
        self.board_panel.render();
    }

    /// Processes a key sent to this panel. This key may simply update the state
    /// and re-render, but it may also emit an action ([`Ok`]), or a [`String`]
    /// to be written as an error ([`Err`]).
    pub fn process_key(&mut self, key: termion::event::Key)
        -> Option<Result<PlayerAction, String>>
    {
        let hand = self.board_panel.game.hand();
        let action = match self.action_panel.process_key(key, hand) {
            Some(PlayerAction::BuyStock { stock }) => {

                // If a buying stock action is being processed, the game should
                // be in the buying stock phase.
                let game = self.board_panel.game.game().unwrap();
                let game = match game.clone().disambiguate() {
                    GameDisambiguation::BuyingStock(g) => g,
                    _ => panic!(),
                };

                match game.check_buy_stock(stock) {
                    Ok(_) => Some(Ok(PlayerAction::BuyStock { stock })),
                    Err(why) => {
                        self.request_action(ActionRequest::BuyStock);
                        Some(Err(why.to_string()))
                    },
                }
            },
            Some(PlayerAction::PlayTile { placement }) => {

                // If a placing tile action is being processed, the game should
                // be in the placing tile phase.
                let game = self.board_panel.game.game().unwrap();
                let game = match game.clone().disambiguate() {
                    GameDisambiguation::PlacingTile(g) => g,
                    _ => panic!(),
                };

                match game.check_tile(placement) {
                    Ok(_) => Some(Ok(PlayerAction::PlayTile { placement })),
                    Err(why) => {
                        if why == IncorrectImplication::ShouldFoundCompany {

                            let game = self.board_panel.game.game().unwrap();
                            let available_companies = CompanyMap::new(&())
                                .map(|company, _| !game.board().company_exists(company));

                            let first_available = available_companies.iter()
                                .filter(|(_, &available)| available)
                                .next();
                            let count = available_companies.iter()
                                .filter(|(_, &available)| available)
                                .count();

                            match count {
                                0 => {
                                    self.request_action(ActionRequest::PlayTile);
                                    Some(Err("No companies available.".to_owned()))
                                },
                                1 => Some(Ok(PlayerAction::PlayTile {
                                    placement: TilePlacement {
                                        tile: placement.tile,
                                        implication: Some(
                                            TilePlacementImplication::FoundsCompany(
                                                first_available.unwrap().0
                                            )
                                        )
                                    }
                                })),
                                _ => {
                                    self.action_panel.request_action(
                                        ActionPanelRequest::FoundCompany {
                                            tile_placed: placement.tile,
                                            available_companies,
                                        },
                                        self.board_panel.game.hand()
                                    );
                                    None
                                }
                            }
                            
                        } else {
                            self.request_action(ActionRequest::PlayTile);
                            Some(Err(why.to_string()))
                        }
                    }
                }
            },
            Some(PlayerAction::ResolveMergeStock { selling, trading, keeping }) => {
                todo!();
            }
            None => None,
        };
        action
    }

    /// Re-renders all the underlying panels. It is better to call
    /// [`render_board_panel`] or [`render_action_panel`] specifically depending
    /// on which is updated.
    pub fn render(&mut self) {
        self.board_panel.render();
        self.action_panel.render(self.board_panel.game.hand());
    }

    /// Resizes the panel and re-renders.
    pub fn resize(&mut self, new_panel: TermPanel) {

        // Decide which way to split the panels
        let (board_display, action_display) = if new_panel.dim().size.0 < new_panel.dim().size.1 {
            new_panel.split_horiz(0.5)
        } else {
            new_panel.split_vert(0.5)
        };

        self.board_panel.resize(board_display);
        self.action_panel.resize(action_display, self.board_panel.game.hand());
    }
}
