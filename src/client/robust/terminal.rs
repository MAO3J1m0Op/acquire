use std::{io::{self, Stdout, Write}, fmt, rc::Rc, cell::RefCell};

use termion::{raw::RawTerminal, event::Key, color::{Color, self}, cursor::HideCursor};
use tokio::sync::mpsc;

use super::panels::PanelDim;

/// Hidden behind a `RefCell` to control the terminal itself.
struct TermControls {
    terminal: HideCursor<RawTerminal<Stdout>>,
}

impl Write for TermControls {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.terminal.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.terminal.flush()
    }

    fn write_vectored(&mut self, bufs: &[io::IoSlice<'_>]) -> io::Result<usize> {
        self.terminal.write_vectored(bufs)
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.terminal.write_all(buf)
    }

    fn write_fmt(&mut self, fmt: fmt::Arguments<'_>) -> io::Result<()> {
        self.terminal.write_fmt(fmt)
    }
}

pub struct TermPanel {
    controls: Rc<RefCell<TermControls>>,
    dim: PanelDim,
}

impl std::fmt::Debug for TermPanel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TermPanel")
            .field("controls", &"...")
            .field("dim", &self.dim)
            .finish()
    }
}

impl TermPanel {
    /// Puts the `stdio` into raw mode, and returns a new `Terminal` instance as
    /// well as a receiver of [`Key`] events.
    pub fn new() -> io::Result<(TermPanel, mpsc::Receiver<Key>)> {
        use termion::raw::IntoRawMode;
        use termion::input::TermRead;

        // Clear the screen
        let mut stdout = io::stdout();
        write!(stdout, "{}", termion::clear::All)?;

        let stdout = HideCursor::from(IntoRawMode::into_raw_mode(stdout)?);
        let (key_sender, key_receiver) = mpsc::channel(1);
        
        std::thread::spawn(move || {

            let stdin = io::stdin();
            let mut stdin = stdin.lock().keys();

            loop {
                let key = stdin.next().unwrap().unwrap();
                let result = key_sender.blocking_send(key);
                
                // SendError means the receiver is closed, ergo this task should end.
                if let Err(_why) = result {
                    break;
                }
            }
        });

        let terminal = TermPanel {
            controls: Rc::new(RefCell::new(TermControls {
                terminal: stdout,
            })),
            dim: PanelDim {
                top_left: (1, 1),
                size: termion::terminal_size()?,
            }
        };

        Ok((terminal, key_receiver))
    }

    /// Tests if a character is writable to the terminal without printing anything.
    pub fn test_char(chr: char) -> Result<(), TermWriteError> {
        match chr {
            ' '..='~' => Ok(()),
            '\n' => Ok(()),
            c => Err(TermWriteError(c))
        }
    }

    pub fn dim(&self) -> PanelDim {
        self.dim
    }

    pub fn write<F, R>(&mut self, overflow_mode: OverflowMode, closure: F) -> R
        where F: FnOnce(&mut TermWriter) -> R
    {
        let mut panel = RefCell::borrow_mut(&self.controls);
        let mut writer = TermWriter::new(self, &mut *panel, overflow_mode);
        closure(&mut writer)
    }

    pub fn fill(&mut self, filler: char) -> Result<(), TermWriteError> {
        self.write(Wrap, |writer| {
            while writer.can_write_char() {
                writer.write_char(filler)?;
            }
            Ok(())
        })?;

        Ok(())
    }

    #[inline]
    pub fn clear(&mut self) {
        self.fill(' ').unwrap();
    }

    pub fn split_horiz(self, weight: f64) -> (Self, Self) {
        let (left, right) = self.dim.split_horiz(weight);
        let left = TermPanel {
            controls: Rc::clone(&self.controls),
            dim: left,
        };
        let right = TermPanel {
            controls: self.controls,
            dim: right,
        };
        (left, right)
    }

    /// Tries to shave columns off of this panel. If the operation succeeds (as
    /// in there isn't an attempt to shave off more columns than this panel
    /// has), returns panels for the left and right columns.
    pub fn shave_horiz(&mut self, off_left: u16, off_right: u16)
        -> Option<(Self, Self)>
    {
        let option = self.dim.shave_horiz(off_left, off_right);
        option.map(|(left, center, right)| {
            self.dim = center;
            let left = TermPanel {
                controls: Rc::clone(&self.controls),
                dim: left,
            };
            let right = TermPanel {
                controls: Rc::clone(&self.controls),
                dim: right,
            };
            (left, right)
        })
    }

    pub fn split_vert(self, weight: f64) -> (Self, Self) {
        let (top, bottom) = self.dim.split_vert(weight);
        let left = TermPanel {
            controls: Rc::clone(&self.controls),
            dim: top,
        };
        let right = TermPanel {
            controls: self.controls,
            dim: bottom,
        };
        (left, right)
    }

    /// Tries to shave rows off of this panel. If the operation succeeds (as
    /// in there isn't an attempt to shave off more rows than this panel
    /// has), returns panels for the left and right rows.
    pub fn shave_vert(&mut self, off_top: u16, off_bottom: u16)
        -> Option<(Self, Self)>
    {
        let option = self.dim.shave_vert(off_top, off_bottom);
        option.map(|(top, center, bottom)| {
            self.dim = center;
            let left = TermPanel {
                controls: Rc::clone(&self.controls),
                dim: top,
            };
            let right = TermPanel {
                controls: Rc::clone(&self.controls),
                dim: bottom,
            };
            (left, right)
        })
    }

    /// Reduces the size of this panel. This operation will fail unless the new
    /// panel dimensions are fully contained within the old panel dimensions.
    pub fn reduce_size(&mut self, new_dim: PanelDim) -> bool {
        let top_left_check = self.dim.top_left < new_dim.top_left;
        let bottom_right_check =
            self.dim.top_left.0 + self.dim.size.0 > new_dim.top_left.0 + new_dim.size.0
            && self.dim.top_left.1 + self.dim.size.1 > new_dim.top_left.1 + new_dim.size.0;

        let success = top_left_check && bottom_right_check;

        if success {
            self.dim = new_dim;
        }
        
        success
    }
}

/// A struct that controls the terminal to write to a panel. This writer assumes
/// control of the entire terminal for its lifetime, so care should be made to
/// ensure that only one writer exists at any given moment.
pub struct TermWriter<'a> {
    panel: &'a TermPanel,
    term: &'a mut TermControls,
    /// The position of the cursor within the panel.
    /// 
    /// # Invariants
    /// 
    /// The `x` position is always contained within the range
    /// `0..=panel.size.0`, and the `y` position is always contained within the
    /// range `0..=panel.size.1.`
    /// 
    /// ## Writable
    /// 
    /// This state is indicated by the position being contained fully within the
    /// bounds specified by the `panel` variable. In this state, characters can
    /// always be added to the buffer regardless of kind. Reaching the end of
    /// the line in `Truncate` mode will place the panel in the `Not Writable -
    /// Full Line` state, and filling up or reaching the bottom line will place
    /// the panel in the `Not Writable - Full Panel` state.
    /// 
    /// ## Writable - Last Line
    /// 
    /// Indicated by the `y` coordinate being equal to `panel.size.1 - 1` (which
    /// is still contained within the bounds of the panel), this state behaves
    /// exactly as the `Writable` state except that new line characters will put
    /// the panel into the `Not Writable - Full Panel` state.
    /// 
    /// ## Not Writable - Full Line
    /// 
    /// This state is only accessible if the overflow mode is set to `Truncate`.
    /// This state is indicated by a `y` position that is fully contained within
    /// the bounds of `panel`, but an `x` position that is equal to
    /// `panel.size.0`. A new line character will advance the cursor to the next
    /// line and put the panel in the `Writable` state, but all other characters
    /// until the new line will be ignored.
    /// 
    /// ## Not Writable - Full Panel
    /// 
    /// This state is indicated by a `y` position that is equal to
    /// `panel.size.1`. In this state, all characters are ignored.
    /// 
    cursor_pos: (u16, u16),
    overflow_mode: OverflowMode,
}

impl<'a> TermWriter<'a> {

    /// Creates a panel and writes the necessary cursor movement positions to
    /// set up this writer.
    fn new(
        panel: &'a TermPanel,
        terminal: &'a mut TermControls,
        overflow_mode: OverflowMode
    ) -> Self {
        let mut val = Self {
            panel,
            term: terminal,
            cursor_pos: (0, 0),
            overflow_mode,
        };
        val.move_cursor((0, 0));
        val
    }

    pub fn overflow_mode(&self) -> OverflowMode {
        self.overflow_mode
    }

    pub fn set_overflow_mode(&mut self, mode: OverflowMode) {
        self.overflow_mode = mode;
    }

    /// Moves the cursor to the specified position within the panel.
    pub fn move_cursor(&mut self, offset: (u16, u16)) {
        let x = self.panel.dim.top_left.0 + offset.0;
        let y = self.panel.dim.top_left.1 + offset.1;
        write!(self.term, "{}", termion::cursor::Goto(x, y)).unwrap();
        self.cursor_pos = offset;
    }

    /// Returns true if the panel is at the end of the current line, thus must
    /// either wrap or await a newline character.
    pub fn end_of_line(&self) -> bool {
        self.cursor_pos.0 == self.panel.dim.size.0
    }

    /// Returns whether this panel has additional space for characters. It is
    /// possible for `can_write_char` to be false and `has_space` to be true if
    /// the panel is in [`OverflowMode::Truncate`] mode, and the current line is
    /// full, but additional lines remain below the current line.
    pub fn has_space(&self) -> bool {
        !(self.cursor_pos.1 == self.panel.dim.size.1)
    }

    /// Returns whether the terminal is in a writable state, or a state that
    /// accepts printable characters.
    pub fn can_write_char(&self) -> bool {
        self.has_space() && !self.end_of_line()
    }

    /// Moves the cursor to the next line. Returns true if the cursor
    /// successfully moved to a new line, and false if the panel is full.
    pub fn new_line(&mut self) -> bool {
        if !self.has_space() { return false; }
        self.move_cursor((0, self.cursor_pos.1 + 1));
        true
    }

    /// Writes a single character to the terminal. The provided text should only
    /// contain ASCII text characters. The only exception is newline, which
    /// moves the cursor to the beginning of the next line in the panel. Returns
    /// true if the character was successfully written.
    pub fn write_char(&mut self, chr: char) -> Result<bool, TermWriteError> {
        debug_assert!(TermPanel::test_char(chr).is_ok());
        match chr {
            // Newline character
            '\n' => {
                Ok(self.new_line())
            },
            // Printable ASCII character
            c @ ' '..='~' => {
                if self.can_write_char() {
                    write!(self.term, "{c}").unwrap();
                    self.move_cursor_right();
                    Ok(true)
                } else { Ok(false) }
            }
            // Error on all other characters
            c => {
                Err(TermWriteError(c))
            }
        }
    }

    /// Writes text to the terminal. The provided text should only contain ASCII
    /// text characters. The only exception is newline, which moves the cursor
    /// to the beginning of the next line in the panel.
    #[inline]
    pub fn write_str(&mut self, text: &str)-> Result<(), TermWriteError> {
        for chr in text.chars() {
            self.write_char(chr)?;
        }

        Ok(())
    }

    pub fn write(&mut self, item: &impl TermRender)-> Result<(), TermWriteError> {
        item.render(self)
    }

    /// Tries to move the cursor to the right, wrapping to the next line if
    /// necessary and permitted. Returns true if successful.
    fn move_cursor_right(&mut self) -> bool {

        // If we can't write a char now, that means we're at the last permitted
        // position, so we can't move the cursor right.
        if !self.can_write_char() { return false; }

        self.move_cursor((self.cursor_pos.0 + 1, self.cursor_pos.1));

        // Handle the end of the line
        if self.end_of_line() {
            match self.overflow_mode {
                OverflowMode::Wrap => {
                    self.new_line()
                },
                OverflowMode::Truncate => {
                    // Do nothing; we can't go any further
                    false
                },
            }
        }
        
        else { true }
    }

    /// Writes text to the terminal in the specified color.
    #[inline]
    pub fn write_colored(&mut self, text: impl TermWritable,
        fg_color: impl Color,
        bg_color: impl Color,
    ) -> Result<(), TermWriteError> {
        write!(self.term, "{}", color::Fg(fg_color)).unwrap();
        write!(self.term, "{}", color::Bg(bg_color)).unwrap();
        text.write(self)?;
        write!(self.term, "{}", color::Fg(color::Reset)).unwrap();
        write!(self.term, "{}", color::Bg(color::Reset)).unwrap();
        Ok(())
    }

    #[inline]
    pub fn write_fg_colored(&mut self, text: impl TermWritable, color: impl Color)
        -> Result<(), TermWriteError>
    {
        write!(self.term, "{}", color::Fg(color)).unwrap();
        text.write(self)?;
        write!(self.term, "{}", color::Fg(color::Reset)).unwrap();
        Ok(())
    }

    pub fn write_bg_colored(&mut self, text: impl TermWritable, color: impl NiceFgColor)
        -> Result<(), TermWriteError>
    {

        struct NiceFgWrapper<'a, C: NiceFgColor>(&'a C);
        impl<'a, C: NiceFgColor> fmt::Display for NiceFgWrapper<'a, C> {
            #[inline]
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.write_nice_fg(f)
            }
        }

        write!(self.term, "{}", NiceFgWrapper(&color)).unwrap();
        write!(self.term, "{}", color::Bg(color)).unwrap();
        text.write(self)?;
        write!(self.term, "{}", color::Fg(color::Reset)).unwrap();
        write!(self.term, "{}", color::Bg(color::Reset)).unwrap();
        Ok(())
    }
}

impl<'a> Drop for TermWriter<'a> {
    fn drop(&mut self) {
        self.term.flush().unwrap();
    }
}

/// Specifies what to do when there is more text to print than can fit on one
/// line within the panel.
#[derive(Debug, Clone, Copy)]
pub enum OverflowMode {
    /// Wrap the text to a new line in the panel
    Wrap,
    /// Stop printing
    Truncate,
}

use OverflowMode::*;

mod sealed {
    pub trait Sealed {}
    impl Sealed for char {}
    impl<'a> Sealed for &'a str {}
}

/// Indicates a failure in writing a character or string to the console.
#[derive(Debug)]
pub struct TermWriteError(char);

impl fmt::Display for TermWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "attempted to print non-printable character: {}",
            self.0.escape_unicode()
        )
    }
}

impl std::error::Error for TermWriteError {}

/// An object that is able to be printed to the terminal.
pub trait TermWritable: sealed::Sealed + Copy {
    fn write(self, term: &mut TermWriter) -> Result<(), TermWriteError> ;
}

impl TermWritable for char {
    #[inline]
    fn write(self, term: &mut TermWriter) -> Result<(), TermWriteError> {
        term.write_char(self)?;
        Ok(())
    }
}

impl<'a> TermWritable for &'a str {
    #[inline]
    fn write(self, term: &mut TermWriter) -> Result<(), TermWriteError> {
        term.write_str(self)?;
        Ok(())
    }
}

pub trait TermRender {
    fn render(&self, term: &mut TermWriter) -> Result<(), TermWriteError>;
}

impl<T: TermWritable> TermRender for T {
    fn render(&self, term: &mut TermWriter) -> Result<(), TermWriteError> {
        TermWritable::write(*self, term)?;
        Ok(())
    }
}

/// When the color appears in the background, this trait gives the ideal color
/// for text overlay.
pub trait NiceFgColor: Color {
    fn write_nice_fg(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}

impl<'a> Color for &'a dyn NiceFgColor {
    fn write_fg(&self, f: &mut fmt::Formatter) -> fmt::Result {
        (*self).write_fg(f)
    }

    fn write_bg(&self, f: &mut fmt::Formatter) -> fmt::Result {
        (*self).write_bg(f)
    }
}

impl<'a> NiceFgColor for &'a dyn NiceFgColor {
    fn write_nice_fg(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (*self).write_nice_fg(f)
    }
}

macro_rules! nice_color {
    ($impler:ty, $c:expr) => {
        impl NiceFgColor for $impler {
            #[inline]
            fn write_nice_fg(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                $c.write_fg(f)
            }
        }
    };
}

nice_color!(color::Reset,       color::Reset);

nice_color!(color::Red,         color::Reset);
nice_color!(color::Blue,        color::Reset);
nice_color!(color::Green,       color::Reset);
nice_color!(color::Magenta,     color::Reset);
nice_color!(color::Yellow,      color::Reset);
nice_color!(color::Cyan,        color::Reset);
nice_color!(color::White,       color::Black);
nice_color!(color::Black,       color::White);

nice_color!(color::LightRed,    color::Reset);
nice_color!(color::LightBlue,   color::Reset);
nice_color!(color::LightGreen,  color::Reset);
nice_color!(color::LightMagenta,color::Reset);
nice_color!(color::LightYellow, color::Reset);
nice_color!(color::LightCyan,   color::Reset);
nice_color!(color::LightWhite,  color::Black);
nice_color!(color::LightBlack,  color::White);
