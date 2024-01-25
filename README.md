# Acquire

Welcome welcome! [Acquire](https://en.wikipedia.org/wiki/Acquire) is an old 1964
board game designed by Sid Sackson, a fan favorite of my grandfather, dad, and
his brothers, and a game to which I was recently introduced at a family
Christmas. I'm working to digitalize the game because 1) I love the Rust
programming language, and needed an excuse to work on something, 2) I wanted to
try my hand at making a tool that takes advantage of the more advanced features
of the UNIX terminal, and 3) I'd like a means to play the board game I love with
friends as we're scattered across the country.

## Progress

This has been a pet project of mine since the beginning of 2023 (college tends
to intrude on progress). It's close to being completely playable, with just a
few gameplay issues. The GUI is also unfinished, but a substitution system
exists using the built-in command prompt. Connecting to other players is
functional, but unpolished, and work will be put into that to make the system
more stable, secure and intuitive for development.

## Usage

After compiling, type `acquire --help` to pull up the list of options:

- `host` is used to host a server on a specified port. Other players will be
  able to connect to the server via TCP through this port. A client GUI will
  open, allowing the host to play the game as well.
- `join` is used to join an existing game of Acquire hosted by someone else at
  the specified IP address. Upon successful connection, this will also
  initialize a client GUI.

### Exiting

In the GUI, press the Esc key, then `y` to confirm exit.

### Commands

In the bottom-right corner of the GUI, there is the option to type commands and
chat messages. To begin a chat message, type the `'>'` key to focus onto the
prompt, and press enter to send your message. To begin a game command, use the
`'/'` key, which will focus onto the prompt with a cyan cursor, and
administrators of a game (including the host) have access to a suite of commands
(in progress) using the `'#'` key, which will give a yellow cursor to the
prompt. The commands are under development, but the current ones are listed
below:

- `play <tile> ...` plays a tile from your hand. The format permitted is x-##, where
  x is the tile's letter in lowercase, and ## is the tile's number.
- `buy <company x3>` is the command used to buy stock. Up to three company names
  can be specified separated by spaces. The first letter of the company is also
  accepted.
- `resolve trade <int> sell <int> keep <int>` is the command used to resolve
  merges. Use the three integer fields to specify how much of your stock you
  wish to trade, sell, or keep. The order of the keywords does not matter, so
  long as they are each followed by a valid integer.

### Admin Commands

- `start` begins a new game.
- `end` immediately ends the game.
- More are coming soon, as alluded by the error message built into the command
  prompt.

## Codebase Tour

I've divided the codebase into five key modules:

- `cli` is a dwarf module that handles the program's command line interface.
- `game` is the core module, storing all the structures that control flow of the
  game itself. Key submodules and objects include:
  - `kernel`, which stores the object that represents the game itself. The way
    this is implemented is rather complex--possibly needlessly so--and subject
    to change.
  - `tile` stores the `Tile` structure and specialized collections thereof,
    including the `Boneyard` and a player's `Hand`.
  - `messages` holds numerous structs and enums that a client and a server use
    to communicate with each other.
- `server` contains the code for facilitating a game of Acquire on
    a server. Much of the game logic code is intentionally placed in the `game`
    module, but some logic may also be presented in this module.
- `client` is a very large module, as it handles both communication with the
  server and contains all the code for the terminal-based GUI that the player
  sees. This module is under heavy development at the moment, and its structure
  is very much subject to change.
- `net` contains the machinery required to connect a client to a server via a
  TCP connection.
