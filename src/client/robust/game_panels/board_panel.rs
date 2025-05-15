use crate::client::robust::terminal::{OverflowMode, TermPanelCache, TermPanelUpdate};
use crate::game::kernel::{self, Game};
use crate::server::ConnectionManager;

/// Renders the board and lobby. Has no state.
pub struct BoardLobbyPanel {
    panel: TermPanelCache,
}

impl BoardLobbyPanel {
    pub fn new(panel: TermPanelUpdate) -> Self {
        Self { panel: panel.into() }
    }

    pub fn draw_board(&mut self, game: Game<kernel::Ambiguous>) {

        // Render the board
        self.panel.clear();
        self.panel.write(OverflowMode::Wrap, |writer| {
            // Print the board
            writer.write(game.board()).unwrap();

            // Print the players
            game.players().iter()
            .for_each(|(player, data)| {
                writer.write_str(&format!("{} ${}\n", player, data.money)).unwrap();
            });
        });
    }

    pub fn draw_lobby(&mut self, connections: &ConnectionManager) {
        // Render the lobby
        self.panel.clear();
        self.panel.write(OverflowMode::Wrap, |writer| {
            // Print the header
            writer.write_fg_colored("PLAYERS", termion::color::LightWhite).unwrap();

            // Print each player
            connections.connections()
                .for_each(|(name, spectating)| {
                    writer.new_line();
                    match spectating {
                        true => {
                            writer.write_fg_colored(&*name, termion::color::Blue)
                        }
                        false => {
                            writer.write_fg_colored(&*name, termion::color::LightBlue)
                        },
                    }.unwrap();
                });
        });
    }
}
