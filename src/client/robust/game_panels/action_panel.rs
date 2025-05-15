use company_panel::{CompanyPanel, CompanyPanelKeyProcessEvent};
use stock_panel::{StockPanel, StockPanelKeyProcessEvent};
use tile_panel::{TilePanel, TilePanelKeyProcessEvent};

use crate::client::robust::panels::PanelTooSmallError;
use crate::client::robust::terminal::{NiceFgColor, OverflowMode, TermPanelUpdate, TermWriteError};
use crate::client::robust::terminal::TermPanelCache;
use crate::game::board::Board;
use crate::game::{messages::*, Company, CompanyMap};
use crate::game::tile::Tile;

mod company_panel;
mod stock_panel;
mod tile_panel;

/// Stores the state and renders whatever menu is in progress.
#[derive(Debug)]
pub struct ActionPanel {
    company_panel: CompanyPanel,
    stock_panel: StockPanel,
    tile_panel: TilePanel,
    state: Option<ActionState>,
    keystroke_demander: KeystrokeDemander,
}

impl ActionPanel {

    /// Creates and renders a new action panel.
    pub fn new(panel: TermPanelUpdate) -> Result<Self, PanelTooSmallError> {

        let split = PanelSplit::new(panel)?;

        Ok(Self {
            company_panel: CompanyPanel::new(split.company_panel),
            stock_panel: StockPanel::new(split.stock_panel),
            tile_panel: TilePanel::new(split.tile_panel)?,
            state: None,
            keystroke_demander: KeystrokeDemander::TilePanel,
        })
    }

    /// Resizes and renders the panel.
    pub fn resize(&mut self, new_panel: TermPanelUpdate) -> Result<(), PanelTooSmallError> {
        let split = PanelSplit::new(new_panel)?;
        let _cp_res = self.company_panel.resize(split.company_panel);
        let _sp_res = self.company_panel.resize(split.stock_panel);
        let tp_res = self.tile_panel.resize(split.tile_panel);
        tp_res
    }

    /// Cancels whatever action was in progress and clears the action panel.
    pub fn cancel_action(&mut self) {

        // Cancels the action in every sub panel
        self.company_panel.conclude_action();
        self.stock_panel.conclude_action();
        self.set_keystroke_demander(KeystrokeDemander::TilePanel);

        self.state = None;
    }

    /// Sets the panel's action to placing tile.
    pub fn request_place_tile(&mut self) {
        self.cancel_action();
        self.state = Some(ActionState::ChoosingTile);
    }

    fn request_found_company(&mut self, tile_placed: Tile, available_companies: CompanyMap<bool>) {
        self.cancel_action();
        self.company_panel.start_action(available_companies, false);
        self.set_keystroke_demander(KeystrokeDemander::CompanyPanel);

        self.state = Some(ActionState::FoundingCompany(tile_placed));
    }

    fn request_merge(&mut self, tile_placed: Tile, merge_tie: MergeTie) {
        self.cancel_action();
        self.company_panel.start_action(merge_tie.participants(), false);
        self.set_keystroke_demander(KeystrokeDemander::CompanyPanel);

        self.state = Some(ActionState::Merging(tile_placed, merge_tie));
    }

    /// Sets the panel's action to buying stock.
    pub fn request_buy_stock(&mut self, available_companies: CompanyMap<bool>) {
        self.cancel_action();
        self.company_panel.start_action(available_companies, true);
        self.stock_panel.start_action();
        self.set_keystroke_demander(KeystrokeDemander::CompanyPanel);
        self.state = Some(ActionState::BuyingStock);
    }

    pub fn request_resolve_merge_stock(&mut self, todo: ()) {
        todo!();
    }

    /// Sets the keystroke demander and highlights/unhighlights the sub-panels as needed.
    fn set_keystroke_demander(&mut self, demander: KeystrokeDemander) {
        match demander {
            KeystrokeDemander::CompanyPanel => {
                self.company_panel.highlight();

                self.tile_panel.unhighlight();
                self.stock_panel.unhighlight();
            },
            KeystrokeDemander::StockPanel => {
                self.stock_panel.highlight();

                self.company_panel.unhighlight();
                self.tile_panel.unhighlight();
            },
            KeystrokeDemander::TilePanel => {
                self.tile_panel.highlight();

                self.company_panel.unhighlight();
                self.stock_panel.unhighlight();
            },
        }
        self.keystroke_demander = demander;
    }

    /// Processes a single key from the user. If that key completes the action,
    /// this function returns [`Some`] with the completed action.
    pub fn process_key(&mut self, key: termion::event::Key, board: &Board) -> Result<Option<ClientMessage>, Box<str>> {
        match self.keystroke_demander {
            KeystrokeDemander::CompanyPanel => {
                let Some(result) = self.company_panel.process_key(key) else {
                    return Ok(None);
                };
                match result {
                    CompanyPanelKeyProcessEvent::EmittedCompany(company) => {
                        let action = self.process_company_emission(company, board);
                        let Some(action) = action else {
                            return Ok(None);
                        };
                        Ok(Some(ClientMessage::TakingTurn(action)))
                    },
                    CompanyPanelKeyProcessEvent::MoveIndexBackward => {

                        // Only process this event when we're buying stock (the stock panel is active)
                        if let Some(ActionState::BuyingStock) = self.state {
                            self.stock_panel.move_index_backward();
                            let company = self.stock_panel.current_index();
                            // Unwrap: we assert the stock panel is only storing
                            // companies available to the company panel
                            self.company_panel.cycle_to_company(company).unwrap();
                        }

                        Ok(None)
                    },
                    CompanyPanelKeyProcessEvent::ExitUpward => {
                        // There's never a panel above the company panel
                        Ok(None)
                    },
                    CompanyPanelKeyProcessEvent::ExitDownward => {
                        if matches!(self.state, Some(ActionState::BuyingStock)) {
                            self.set_keystroke_demander(KeystrokeDemander::StockPanel);
                        } else {
                            self.set_keystroke_demander(KeystrokeDemander::TilePanel);
                        }
                        Ok(None)
                    },
                }
            },
            KeystrokeDemander::StockPanel => {
                let Some(result) = self.stock_panel.process_key(key) else {
                    return Ok(None);
                };
                match result {
                    StockPanelKeyProcessEvent::ExitUpward => {
                        // The company panel is always above the stock panel
                        self.set_keystroke_demander(KeystrokeDemander::CompanyPanel);
                        Ok(None)
                    },
                    StockPanelKeyProcessEvent::ExitDownward => {
                        // The tile panel is always below the stock panel
                        self.set_keystroke_demander(KeystrokeDemander::TilePanel);
                        Ok(None)
                    },
                    StockPanelKeyProcessEvent::CycleToCompany(company) => {
                        // Unwrap: we assert the stock panel is only storing
                        // companies available to the company panel
                        self.company_panel.cycle_to_company(company).unwrap();
                        Ok(None)
                    },
                }
            },
            KeystrokeDemander::TilePanel => {
                let Some(result) = self.tile_panel.process_key(key) else {
                    return Ok(None);
                };
                match result {
                    TilePanelKeyProcessEvent::TileChosen { chosen, cached_annotation } => {
                        Ok(self.process_tile_event(chosen, cached_annotation, board))
                    },
                    TilePanelKeyProcessEvent::ExitUpward => {
                        if matches!(self.state, Some(ActionState::BuyingStock)) {
                            self.set_keystroke_demander(KeystrokeDemander::StockPanel);
                        }
                        if matches!(self.state, Some(ActionState::FoundingCompany(_))) {
                            self.set_keystroke_demander(KeystrokeDemander::CompanyPanel);
                        }
                        if matches!(self.state, Some(ActionState::ResolvingMergeStock)) {
                            todo!();
                        }

                        Ok(None)
                    },
                    TilePanelKeyProcessEvent::ExitDownward => {
                        // Do nothing; there is never a panel below the tile panel
                        Ok(None)
                    },
                }
            },
        }
    }

    /// The tile panel emits a tile.
    fn process_tile_event(&mut self,
        tile: Tile,
        cached_annotation: Option<IncorrectImplication>,
        board: &Board,
    ) -> Option<ClientMessage> {

        // Dead tiles can be exchanged regardless of whether it is your turn
        if cached_annotation == Some(IncorrectImplication::DeadTile) {
            return Some(ClientMessage::DeadTile { dead_tile: tile });
        }

        // The tile panel is always on display, so any action state is possible.
        if let Some(ActionState::ChoosingTile) = &self.state {
            let emitted_message = match cached_annotation {
                Some(IncorrectImplication::BadDefunctOrder) => {
                    panic!("not stored in AnnotatedHandEntry")
                },
                Some(IncorrectImplication::CompanyTaken) => {
                    panic!("not stored in AnnotatedHandEntry")
                },
                Some(IncorrectImplication::DeadTile) => {
                    panic!("case handled earlier")
                },
                Some(IncorrectImplication::IncorrectDefunct(_)) => {
                    panic!("not stored in AnnotatedHandEntry")
                },
                Some(IncorrectImplication::LargeIntoSmall) => {
                    panic!("not stored in AnnotatedHandEntry")
                },
                Some(IncorrectImplication::MissedDefunct(_)) => {
                    panic!("not stored in AnnotatedHandEntry")
                },
                Some(IncorrectImplication::ShouldBeNone) => {
                    panic!("not stored in AnnotatedHandEntry")
                },
                Some(IncorrectImplication::ShouldFoundCompany) => {
                    self.request_found_company(tile, board.available_companies());
                    None
                },
                Some(IncorrectImplication::ShouldMerge) => {
                    let participants = board.merge_participants(tile);
                    match Merge::make_merge(participants, board.company_sizes) {
                        Ok(merge) => {
                            self.cancel_action();
                            Some(ClientMessage::TakingTurn(
                                PlayerAction::PlayTile {
                                    placement: TilePlacement {
                                        tile,
                                        implication: Some(TilePlacementImplication::MergesCompanies(merge))
                                    }
                                }
                            ))
                        },
                        Err(merge_tie) => {
                            self.request_merge(tile, merge_tie);
                            None
                        },
                    }
                }
                None => todo!(),
            };

            return emitted_message;
        }

        None
    }

    /// The company panel emits a company
    fn process_company_emission(&mut self, company: Option<Company>, board: &Board) -> Option<PlayerAction> {
        // Unwrap: we shouldn't have a None state if we're emitting a company
        match self.state.take().unwrap() {
            ActionState::ChoosingTile => {
                panic!("company panel shouldn't be active");
            },
            ActionState::BuyingStock => {

                self.stock_panel.set_current_index(company);

                if !self.stock_panel.move_index_forward() {
                    let stock = *self.stock_panel.stock();
                    self.cancel_action();
                    Some(PlayerAction::BuyStock { stock })
                }

                // If the index push was successful, we haven't moved all the way right yet
                else {

                    // We called take() on state earlier, so we have to reset it
                    self.state = Some(ActionState::BuyingStock);

                    None
                }
            },
            ActionState::FoundingCompany(_) => {
                panic!("company panel shouldn't be active");
            },
            ActionState::Merging(tile, merge_tie) => {
                // Unwrap: merge state disallows None in the company panel
                let company = company.unwrap();
                match merge_tie.advance(company, board.company_sizes) {
                    Ok(merge) => {
                        let action = PlayerAction::PlayTile {
                            placement: TilePlacement {
                                tile,
                                implication: Some(TilePlacementImplication::MergesCompanies(merge)),
                            },
                        };
                        // Emit the merge
                        Some(action)
                    },
                    Err(tie) => {
                        // Set the state with the new tie
                        self.state = Some(ActionState::Merging(tile, tie));
                        None
                    },
                }
            },
            ActionState::ResolvingMergeStock => {
                panic!("company panel shouldn't be active");
            },
        }
    }
}

/// Used to track the internal state of the action panel.
#[derive(Debug)]
enum ActionState {
    /// Player is choosing a tile to play.
    ChoosingTile,
    /// Player is buying stock.
    BuyingStock,
    /// Player is choosing a company to found.
    FoundingCompany(Tile),
    /// Player is working on breaking ties in a merge.
    Merging(Tile, MergeTie),
    /// Player is choosing what to do with their stock tied up in a merger.
    ResolvingMergeStock,
}

/// Determines which of the action panel sub-panels is highlighted and therefore
/// should be forwarded keystrokes.
#[derive(Debug)]
enum KeystrokeDemander {
    CompanyPanel,
    StockPanel,
    TilePanel,
}

/// Splits the provided [`TermPanel`] for this action panel into sub-panels.
#[derive(Debug)]
struct PanelSplit<'c> {
    pub company_panel: TermPanelUpdate<'c>,
    pub stock_panel: TermPanelUpdate<'c>,
    pub tile_panel: TermPanelUpdate<'c>,
    _private_constructor: (),
}

impl<'c> PanelSplit<'c> {
    pub fn new(panel: TermPanelUpdate<'c>) -> Result<Self, PanelTooSmallError> {


        Ok(Self {
            company_panel: todo!(),
            stock_panel: todo!(),
            tile_panel: todo!(),
            _private_constructor: (),
        })
    }
}

#[derive(Debug)]
struct TextPanel {
    panel: TermPanelCache,
}

impl TextPanel {
    pub fn new(panel: TermPanelUpdate) -> Self {
        Self { panel: panel.into() }
    }

    pub fn display_text(&mut self, text: &str, color: impl NiceFgColor) -> Result<(), TermWriteError> {
        self.panel.clear();
        self.panel.write(OverflowMode::Wrap, |writer| {
            writer.write_fg_colored(text, color)
        })
    }
}
