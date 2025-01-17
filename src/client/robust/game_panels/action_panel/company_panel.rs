use std::fmt;

use termion::event::Key;
use thiserror::Error;

use crate::{client::robust::terminal::{OverflowMode, TermPanel, TermWriter}, game::{Company, CompanyMap}};

/// Stores the display (if any) that guides the player through choosing a
/// founding company or buying stock.
#[derive(Debug)]
pub struct CompanyPanel {
    panel: TermPanel,
    /// The company chooser, if one exists. Set to [`None`] if the panel should be empty.
    chooser: Option<CompanyChooser>,
    /// True if the user is hovering over anything in this panel (for rendering
    /// purposes).
    highlighted: bool,
}

/// An event that occurs as the result of processing a keystroke in a panel.
pub enum CompanyPanelKeyProcessEvent {
    /// The player pressed a key to emit the company.
    EmittedCompany(Option<Company>),
    /// This is emitted when the user presses a hotkey in this panel to move the
    /// index backwards. This is processed by the action panel and is ignored
    /// unless the panel is in the buying stock state.
    MoveIndexBackward,
    /// The player moved their cursor upwards to exit the panel.
    ExitUpward,
    /// The player moved their cursor downwards to exit the panel.
    ExitDownward,
}

#[derive(Debug, Error)]
pub struct CompanyNotFoundError;

impl fmt::Display for CompanyNotFoundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "company not found")
    }
}


impl CompanyPanel {
    pub fn new(panel: TermPanel) -> Self {
        let mut me = Self {
            panel,
            chooser: None,
            highlighted: false,
        };
        me.render();
        me
    }

    pub fn resize(&mut self, new_panel: TermPanel) {
        self.panel = new_panel;
        self.render();
    }

    /// Initializes the company chooser with the selected available companies.
    pub fn start_action(&mut self, available_companies: CompanyMap<bool>, include_null: bool) {
        self.chooser = Some(CompanyChooser::new(available_companies.true_companies(), include_null));
        self.render();
    }

    /// Concludes the action, emitting the selected company.
    pub fn conclude_action(&mut self) -> Option<Company> {
        let chooser = self.chooser.take()?;
        self.render();
        chooser.selected_company()
    }

    pub fn selected_company(&self) -> Option<Company> {
        self.chooser.as_ref()?.selected_company()
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

    /// Cycles to a specific company. Returns [`Err`] if that company is not in
    /// the chooser, or if this panel is inactive.
    pub fn cycle_to_company(&mut self, company: Option<Company>) -> Result<(), CompanyNotFoundError> {

        let Some(chooser) = self.chooser.as_mut() else {
            return Err(CompanyNotFoundError);
        };

        for (idx, selected) in chooser.included_companies.iter().enumerate() {
            if *selected == company {
                chooser.selected = idx as i8;
                self.render();
                return Ok(())
            }
        }

        Err(CompanyNotFoundError)
    }

    /// Processes a keystroke directed to this panel. If the keystroke results
    /// in the completion of the underlying action, a [`Some`] is returned with
    /// that completed action provided.
    pub fn process_key(&mut self, key: Key) -> Option<CompanyPanelKeyProcessEvent> {

        // Short circuit key processing if this panel isn't rendered
        let Some(chooser) = self.chooser.as_mut() else {
            return None;
        };

        let event = match key {
            Key::Up => {
                Some(CompanyPanelKeyProcessEvent::ExitUpward)
            }
            Key::Down => {
                Some(CompanyPanelKeyProcessEvent::ExitDownward)
            }
            Key::Left => {
                chooser.cycle_left();
                None
            },
            Key::Right => {
                chooser.cycle_right();
                None
            },
            Key::Char('\n') => {
                Some(CompanyPanelKeyProcessEvent::EmittedCompany(chooser.selected_company()))
            },
            // Moves the buying stock cursor back one
            Key::Backspace => {
                Some(CompanyPanelKeyProcessEvent::MoveIndexBackward)
            }
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

        // The panel is empty if `self.chooser` is None.
        let Some(chooser) = self.chooser.as_ref() else {
            return;
        };

        self.panel.write(OverflowMode::Truncate, |writer| {
            write_company_chooser(writer, chooser.selected_company());
        })
    }
}

/// Contains the data required to track a company selector box.
#[derive(Debug)]
struct CompanyChooser {
    /// The array of companies included
    included_companies: [Option<Company>; 8],
    /// Specifies which company is being selected.
    selected: i8,
    /// Reference variable for the number of elements being selected
    length: i8,
}

impl CompanyChooser {
    /// Creates a new CompanyChooser, expecting `available_companies`
    pub fn new(available_companies: impl IntoIterator<Item = Company>, includes_null: bool) -> Self {
        let mut included_companies = [None; 8];

        let mut last_i = 0;
        for (i, company) in available_companies.into_iter().enumerate() {
            last_i += 1;
            included_companies[i] = Some(company)
        }

        let mut length = last_i as i8;
        if includes_null { length += 1; }

        debug_assert!(length != 0, "Constructed an empty CompanyChooser");

        Self {
            included_companies,
            selected: 0,
            length,
        }
    }

    pub fn cycle_left(&mut self) {
        self.selected = (self.selected - 1).rem_euclid(self.length);
    }

    pub fn cycle_right(&mut self) {
        self.selected = (self.selected + 1).rem_euclid(self.length);
    }

    pub fn selected_company(&self) -> Option<Company> {
        self.included_companies[self.selected as usize]
    }
}

fn write_company_chooser(writer: &mut TermWriter, company: Option<Company>) {
    writer.write_str("    < ").unwrap();
    let string = company.map(|c| c.to_string()).unwrap_or("---".to_owned());
    writer.write_bg_colored(&*string, Company::option_color(company)).unwrap();
    writer.write_str(" >\n").unwrap();
}
