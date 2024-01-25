use crate::client::robust::terminal::{TermPanel, OverflowMode};
use crate::client::ClientGame;
use crate::server::ConnectionManager;

pub struct BoardPanel<'c> {
    panel: Option<TermPanel>,
    /// Stores the game in progress.
    pub game: ClientGame,
    pub connections: &'c mut ConnectionManager,
}

impl<'c> BoardPanel<'c> {
    /// Creates a new board panel of size zero. It must be resized later.
    pub fn new(
        game: ClientGame,
        connections: &'c mut ConnectionManager
    ) -> Self {
        Self {
            panel: None,
            game,
            connections,
        }
    }

    pub fn render(&mut self) {
        if let Some(ref mut panel) = self.panel {
            if let Some(game) = self.game.game() {

                // Render the board
                panel.clear();
                panel.write(OverflowMode::Wrap, |writer| {
                    // Print the board
                    writer.write(game.board()).unwrap();

                    // Print the players
                    game.players().iter()
                    .for_each(|(player, data)| {
                        writer.write_str(&format!("{} ${}\n", player, data.money)).unwrap();
                    });
                });

            } else {

                // Render the lobby
                panel.clear();
                panel.write(OverflowMode::Wrap, |writer| {
                    // Print the header
                    writer.write_fg_colored("PLAYERS", termion::color::LightWhite).unwrap();

                    // Print each player
                    self.connections.connections()
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
    }

    /// Resizes and renders the panel.
    pub fn resize(&mut self, new_panel: TermPanel) {
        self.panel = Some(new_panel);
        self.render();
    }
}
