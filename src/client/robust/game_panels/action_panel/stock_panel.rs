use termion::event::Key;

use crate::{client::robust::terminal::{OverflowMode, TermPanel}, game::Company};

/// Stores the display (if any) that guides the player through choosing a
/// founding company or buying stock.
#[derive(Debug)]
pub struct StockPanel {
    panel: TermPanel,
    /// Stores which action is in progress, if any.
    stock: [Option<Company>; 3],
    /// The index hovered over by the player. [`None`] indicates that this panel is inactive.
    highlighted_index: Option<usize>,
    /// True if the player is highlighted in this panel. Needed for rendering purposes.
    highlighted: bool,
}

/// An event that occurs as the result of processing a keystroke in a panel.
#[derive(Debug)]
pub enum StockPanelKeyProcessEvent {
    /// The player moved their cursor upwards to exit the panel.
    ExitUpward,
    /// The player moved their cursor downwards to exit the panel.
    ExitDownward,
    /// The company chooser panel should be cycled to the enclosed company, as
    /// that is what was highlighted in the stock panel.
    CycleToCompany(Option<Company>),
}

impl StockPanel {
    pub fn new(panel: TermPanel) -> Self {
        let mut me = Self {
            panel,
            stock: [None; 3],
            highlighted_index: None,
            highlighted: false,
        };
        me.render();
        me
    }

    pub fn resize(&mut self, new_panel: TermPanel) {
        self.panel = new_panel;
        self.render();
    }

    /// Starts a new stock purchase action. This is different from having the cursor in this panel.
    pub fn start_action(&mut self) {
        self.highlighted_index = Some(0);
        self.render();
    }

    /// Informs the panel that it has been highlighted by the user, for rendering purposes.
    pub fn highlight(&mut self) {
        self.highlighted = true;
        self.render();
    }

    /// Informs the panel that the cursor has jumped out of the panel, for rendering purposes.
    pub fn unhighlight(&mut self) {
        self.highlighted = false;
        self.render();
    }

    pub fn stock(&mut self) -> &[Option<Company>; 3] {
        &self.stock
    }

    /// Completes whatever action is working in this panel and retrieves the stock.
    pub fn conclude_action(&mut self) {
        self.stock = [None; 3];
        self.highlighted_index = None;
        self.render();
    }

    pub fn current_index(&self) -> Option<Company> {
        self.stock[self.highlighted_index?]
    }

    /// Sets the current index. Does nothing if there is no action.
    pub fn set_current_index(&mut self, company: Option<Company>) {
        let Some(idx) = self.highlighted_index else { return };
        self.stock[idx] = company;
        self.render();
    }

    fn move_index_forward_no_render(&mut self) -> bool {
        let Some(idx) = self.highlighted_index.as_mut() else { return false };
        if *idx < 2 {
            *idx += 1;
            true
        } else {
            false
        }
    }

    fn move_index_backward_no_render(&mut self) -> bool {
        let Some(idx) = self.highlighted_index.as_mut() else { return false };
        if *idx > 0 {
            *idx -= 1;
            true
        } else {
            false
        }
    }

    /// Attempts to move the index forward; returns true if successful.
    pub fn move_index_forward(&mut self) -> bool {
        let result = self.move_index_forward_no_render();
        self.render();
        result
    }

    /// Attempts to move the index backward; returns true if successful.
    pub fn move_index_backward(&mut self) -> bool {
        let result = self.move_index_backward_no_render();
        self.render();
        result
    }

    /// Processes a keystroke directed to this panel. If the keystroke results
    /// in the completion of the underlying action, a [`Some`] is returned with
    /// that completed action provided.
    pub fn process_key(&mut self, key: Key) -> Option<StockPanelKeyProcessEvent> {

        let Some(idx) = self.highlighted_index.as_mut() else {
            return None;
        };

        // We don't process keys if the player is not hovering over us
        if !self.highlighted { return None; }

        let event = match key {
            Key::Up => {
                Some(StockPanelKeyProcessEvent::ExitUpward)
            }
            Key::Down => {
                Some(StockPanelKeyProcessEvent::ExitDownward)
            }
            Key::Left => {
                if *idx > 0 {
                    *idx -= 1;
                    Some(StockPanelKeyProcessEvent::CycleToCompany(self.stock[*idx]))
                } else {
                    None
                }
            },
            Key::Right => {
                if *idx < 2 {
                    *idx += 1;
                    Some(StockPanelKeyProcessEvent::CycleToCompany(self.stock[*idx]))
                } else {
                    None
                }
            },
            // Return to short circuit rerendering
            _ => return None,
        };

        // Rerender since if we reach this point a valid key was processed
        self.render();

        event
    }

    /// Redraws the whole panel.
    fn render(&mut self) {
        self.panel.clear();

        let Some(idx) = self.highlighted_index else {
            return;
        };

        self.panel.write(OverflowMode::Truncate, |writer| {
            writer.write_str("  [").unwrap();

            // Write the starting letter of each company
            for (curr_idx, purchase) in self.stock.iter().enumerate() {
                let letter = purchase.map_or('-', |company| { company.char() });
                let color = Company::option_color(*purchase);
                if self.highlighted && curr_idx == idx {
                    // Unwrap ok since letter char is always writable
                    writer.write_bg_colored(letter, color).unwrap();
                } else {
                    // Unwrap ok since letter char is always writable
                    writer.write_fg_colored(letter, color).unwrap();
                }
            }

            writer.write_str("]").unwrap();
        });
    }
}
