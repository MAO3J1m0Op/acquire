use company_panel::{CompanyPanel, CompanyPanelKeyProcessEvent};
use stock_panel::{StockPanel, StockPanelKeyProcessEvent};
use tile_panel::{TilePanel, TilePanelKeyProcessEvent};

use crate::client::robust::panels::PanelTooSmallError;
use crate::client::robust::terminal::{NiceFgColor, OverflowMode, TermWriteError};
use crate::client::robust::terminal::TermPanel;
use crate::game::{messages::*, Company, CompanyMap};
use crate::game::tile::{Hand, Tile};

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
    pub fn new(panel: TermPanel) -> Result<Self, PanelTooSmallError> {

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
    pub fn resize(&mut self, new_panel: TermPanel) -> Result<(), PanelTooSmallError> {
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
    pub fn process_key(&mut self, key: termion::event::Key) -> Option<ClientMessage> {
        match self.keystroke_demander {
            KeystrokeDemander::CompanyPanel => {
                match self.company_panel.process_key(key)? {
                    CompanyPanelKeyProcessEvent::EmittedCompany(company) => {
                        self.process_company_emission(company)
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

                        None
                    },
                    CompanyPanelKeyProcessEvent::ExitUpward => {
                        // There's never a panel above the company panel
                        None
                    },
                    CompanyPanelKeyProcessEvent::ExitDownward => {
                        if matches!(self.state, Some(ActionState::BuyingStock)) {
                            self.set_keystroke_demander(KeystrokeDemander::StockPanel);
                        } else {
                            self.set_keystroke_demander(KeystrokeDemander::TilePanel);
                        }
                        None
                    },
                }
            },
            KeystrokeDemander::StockPanel => {
                match self.stock_panel.process_key(key)? {
                    StockPanelKeyProcessEvent::ExitUpward => {
                        // The company panel is always above the stock panel
                        self.set_keystroke_demander(KeystrokeDemander::CompanyPanel);
                        None
                    },
                    StockPanelKeyProcessEvent::ExitDownward => {
                        // The tile panel is always below the stock panel
                        self.set_keystroke_demander(KeystrokeDemander::TilePanel);
                        None
                    },
                    StockPanelKeyProcessEvent::CycleToCompany(company) => {
                        // Unwrap: we assert the stock panel is only storing
                        // companies available to the company panel
                        self.company_panel.cycle_to_company(company).unwrap();
                        None
                    },
                }
            },
            KeystrokeDemander::TilePanel => {
                match self.tile_panel.process_key(key)? {
                    TilePanelKeyProcessEvent::TileChosen { chosen, cached_annotation } => {
                        self.process_tile_event(chosen, cached_annotation)
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

                        None
                    },
                    TilePanelKeyProcessEvent::ExitDownward => {
                        // Do nothing; there is never a panel below the tile panel
                        None
                    },
                }
            },
        }
    }

    /// The tile panel emits a tile.
    fn process_tile_event(&mut self, tile: Tile, cached_annotation: Option<IncorrectImplication>) -> Option<ClientMessage> {
        todo!()
    }

    /// The company panel emits a company
    fn process_company_emission(&mut self, company: Option<Company>) -> Option<ClientMessage> {
        todo!()
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
    Merging(Tile, ()),
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
struct PanelSplit {
    pub company_panel: TermPanel,
    pub stock_panel: TermPanel,
    pub tile_panel: TermPanel,
    _private_constructor: (),
}

impl PanelSplit {
    pub fn new(panel: TermPanel) -> Result<Self, PanelTooSmallError> {


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
    panel: TermPanel,
}

impl TextPanel {
    pub fn new(panel: TermPanel) -> Self {
        Self { panel }
    }

    pub fn display_text(&mut self, text: &str, color: impl NiceFgColor) -> Result<(), TermWriteError> {
        self.panel.clear();
        self.panel.write(OverflowMode::Wrap, |writer| {
            writer.write_fg_colored(text, color)
        })
    }
}
