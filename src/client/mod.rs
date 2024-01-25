use crate::game::{messages::*, Company, tile::Hand};
use crate::game::kernel::{Game, self, GameDisambiguation};
use crate::game::tile::Tile;
use crate::server::Handshake;

pub mod primitive;
pub mod robust;

/// Decodes a text command into a [`PlayerActionKind`].
pub fn parse_game_command(command: &str) -> Result<PlayerAction, CommandParseErr> {
    use CommandParseErr::*;

    let mut tokens = command.split(" ");

    match tokens.next() {
        // Play a tile
        Some("play") => {
            let tile_str = tokens.next().ok_or(Expected("tile"))?;
            let tile: Tile = tile_str.parse().or(Err(Expected("tile")))?;

            let implication = match tokens.next() {
                None => None,
                Some("founding") => {
                    let token = tokens.next().ok_or(Expected("company"))?;
                    let company = token.parse::<Company>()
                        .or(Err(Expected("company")))?;
                    Some(TilePlacementImplication::FoundsCompany(company))
                },
                Some("merging") => {
                    let mut defunct = vec![];
                    loop {
                        let company: Company = match tokens.next() {
                            Some("into") => break,
                            Some(cmp_string) => cmp_string.parse().or(Err(Expected("company")))?,
                            None => return Err(Expected("into")),
                        };
                        defunct.push(company);
                    }
                    let into: Company = match tokens.next() {
                        Some(cmp_string) => cmp_string.parse().or(Err(Expected("company")))?,
                        None => return Err(Expected("company")),
                    };

                    Some(TilePlacementImplication::MergesCompanies(Merge::new(&defunct, into)))
                }
                Some(_) => return Err(Expected("\"founding\" or \"merging\"")),
            };

            Ok(PlayerAction::PlayTile { placement: TilePlacement { tile, implication }})
        },
        // Buy stock
        Some("buy") => {
            let mut stock = [None::<Company>; 3];
            for share in &mut stock {
                match tokens.next() {
                    Some(company) => {
                        *share = Some(company.parse().or(Err(Expected("company")))?);
                    },
                    None => break,
                }
            }
            Ok(PlayerAction::BuyStock { stock })
        },
        Some("resolve") => {
            let mut sell = None;
            let mut trade = None;
            let mut keep = None;

            for _ in 0..3 {
                let keyword_error = Expected("\"trade\", \"sell\", or \"keep\"");
                let keyword = tokens.next()
                    .ok_or(keyword_error.clone())?;
                let count: u8 = tokens.next().map(|token| token.parse().ok())
                    .flatten()
                    .ok_or(Expected("integer"))?;
                let option = match keyword {
                    "sell" => &mut sell,
                    "trade" => &mut trade,
                    "keep" => &mut keep,
                    _ => return Err(keyword_error),
                };
                match option {
                    Some(_) => return Err(DuplicateArgument(keyword.to_owned())),
                    None => *option = Some(count),
                }
            }
            Ok(PlayerAction::ResolveMergeStock {
                selling: sell.unwrap(),
                trading: trade.unwrap(),
                keeping: keep.unwrap() 
            })
        },
        Some(_) => return Err(Expected("\"play\", \"buy\", or \"resolve\"")),
        None => return Err(EmptyInput),
    }
}

pub fn parse_admin_command(command: &str) -> Result<AdminCommand, CommandParseErr> {
    use CommandParseErr::*;
    let command_message = "\"shutdown\", \"silencechat\", \"start\", \"end\", or \"kick\"";

    Ok(match command {
        "shutdown" => AdminCommand::Shutdown,
        "silencechat" => AdminCommand::SilenceChat,
        "start" => AdminCommand::StartGame,
        "end" => AdminCommand::EndGame,
        other => {
            let space = other.find(" ");
            let space = match space {
                Some(v) => v,
                None => {
                    return if command == "kick" {
                        Err(Expected("player name"))
                    } else {
                        Err(Expected(command_message))
                    };
                },
            };
            let name = &command[(space+1)..];
            AdminCommand::Kick { player_name: name.to_owned().into_boxed_str() }
        }
    })
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum CommandParseErr {
    #[error("empty command")]
    EmptyInput,
    #[error("expected {0}")]
    Expected(&'static str),
    #[error("argument \"{0}\" appeared twice")]
    DuplicateArgument(String),
}

/// Handles the client side of a game. Tracks when a game is in progress and
/// when it is not.
pub struct ClientGame {
    client: Handshake,
    _impl: Option
    <ClientGameImpl>,
}

struct ClientGameImpl {
    game: Game<kernel::Ambiguous>,
    hand: Option<Hand>,
}

impl ClientGame {

    /// Creates a new [`ClientGame`] with no game in progress.
    pub fn new(client: Handshake, history: Option<GameHistory>) -> Self {

        let _impl = history.map(|history| {
            let game: Game<kernel::Ambiguous> = Game::start(&history.start).into();
            let game = game.speed_play(history.actions.into_vec()).unwrap().unwrap();
            ClientGameImpl {
                game,
                hand: None,
            }
        });

        Self { client, _impl }
    }

    /// Starts a new game.
    /// 
    /// # Panics
    /// 
    /// This function assumes that the server knows the
    /// state, and it will panic if the server requests to start a game when one
    /// is already in progress.
    pub fn start(&mut self, game: Game<kernel::Ambiguous>, hand: Option<Hand>) {
        assert!(self._impl.as_ref().is_none(),
            "Server requested the start of a game when one is already in progress"
        );
        self._impl = Some(ClientGameImpl { game, hand });
    }

    /// Updates the client's game. Returns a mutable reference to the new game.
    /// 
    /// # Panics
    /// 
    /// This function assumes that all the validation work has been properly
    /// done server-side. Any unexpected/invalid [`PlayerAction`] will panic.
    /// 
    /// TODO: remove this panic condition and return a proper error.
    pub fn update(&mut self, action: &TaggedPlayerAction) {
        
        // Take ownership of this instance's game
        let game_impl = self._impl.take()
            .expect("Game updated when not in progress.");
        let game = game_impl.game;
        let mut hand = game_impl.hand;

        let new_game: Option<Game<kernel::Ambiguous>> = match &action.action {
            PlayerAction::PlayTile { placement } => {
    
                // The game should be in the placing tile phase
                let stated_game = match game.disambiguate() {
                    GameDisambiguation::PlacingTile(g) => g,
                    _ => panic!("received tile placement when not in placing tile mode"),
                };
    
                let advancer = stated_game.check_player_action(action)
                    .expect("Incorrect move received from server");

                // Update the player's hand if the client placed the tile
                if &action.player_name == &self.client.player_name {
                    let success = hand.as_mut()
                        .expect("Spectator placed a tile")
                        .remove_tile(placement.tile);
                    assert!(success);
                }

                Some(stated_game.advance_game(advancer).into())
            },
            PlayerAction::BuyStock{ stock: _ } => {
    
                // The game should be in buying stock phase
                let stated_game = match game.disambiguate() {
                    GameDisambiguation::BuyingStock(g) => g,
                    _ => panic!("received stock when not in buying stock mode"),
                };
    
                let advancer = stated_game.check_player_action(action)
                    .expect("Invalid purchase received from server");
                stated_game.advance_game(advancer).ok().map(|g| g.into())
            },
            PlayerAction::ResolveMergeStock {
                selling: _,
                trading: _,
                keeping: _ 
            } => {
    
                // The game should be in resolving merge phase
                let stated_game = match game.disambiguate() {
                    GameDisambiguation::ResolvingMerge(game) => game,
                    _ => panic!("Received merge resolution when not in buying stock mode"),
                };
    
                let advancer = stated_game.check_player_action(&action)
                    .expect("Invalid merge resolution received from server");
                stated_game.step_merge(advancer).ok()
            },
        };

        // Put the game, mutated or not, back into this game object. Note to
        // future Will: when implementing a proper error for this function,
        // don't use the ? operator to return, otherwise the game is dropped, as
        // it is owned by a local variable until this is called.
        self._impl = new_game.map(|game| {
            ClientGameImpl { game, hand }
        });
    }

    /// Ends this game.
    /// 
    /// # Panics
    /// 
    /// Panics if there's no game to end.
    pub fn end(&mut self) -> Game<GameOver> {
        let game_impl = self._impl.take()
            .expect("called end() on a ClientGame not in progress");
        game_impl.game.end_early()
    }
    
    pub fn game(&self) -> Option<&Game<kernel::Ambiguous>> {
        self._impl.as_ref().map(|i| &i.game)
    }

    pub fn game_mut(&mut self) -> Option<&mut Game<kernel::Ambiguous>> {
        self._impl.as_mut().map(|i| &mut i.game)
    }

    pub fn hand(&self) -> Option<&Hand> {
        self._impl.as_ref().map(|i| i.hand.as_ref()).flatten()
    }

    pub fn hand_mut(&mut self) -> Option<&mut Hand> {
        self._impl.as_mut().map(|i| i.hand.as_mut()).flatten()
    }
}
