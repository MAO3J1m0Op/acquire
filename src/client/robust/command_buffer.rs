use std::fmt;

use termion::event::Key;

use super::terminal::{TermPanel, NiceFgColor, OverflowMode, TermWriteError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BufferMode {
    /// The user is typing a chat message.
    Chat,
    /// The user is typing a game command.
    Command,
    /// The user is typing an admin command.
    Admin,
}

impl BufferMode {
    pub fn symbol(&self) -> char {
        match self {
            BufferMode::Chat => '>',
            BufferMode::Command => '/',
            BufferMode::Admin => '#',
        }
    }
}

impl termion::color::Color for BufferMode {
    fn write_fg(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use termion::color::*;
        match self {
            BufferMode::Chat => White.write_fg(f),
            BufferMode::Command => Cyan.write_fg(f),
            BufferMode::Admin => Yellow.write_fg(f),
        }
    }
    
    fn write_bg(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use termion::color::*;
        match self {
            BufferMode::Chat => White.write_bg(f),
            BufferMode::Command => Blue.write_bg(f),
            BufferMode::Admin => Yellow.write_bg(f),
        }
    }
}

impl NiceFgColor for BufferMode {
    fn write_nice_fg(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use termion::color::*;
        match self {
            BufferMode::Chat => White.write_nice_fg(f),
            BufferMode::Command => Blue.write_nice_fg(f),
            BufferMode::Admin => Yellow.write_nice_fg(f),
        }
    }
}

/// Methods for editing a buffer using a stream of keys.
#[derive(Debug)]
pub(super) struct CommandBuffer {
    /// The characters stored in this buffer
    buffer: String,
    /// The panel of the terminal where this command buffer sits.
    panel: Option<TermPanel>,
    /// Position of the cursor within the buffer
    cursor_pos: usize,
    /// Decides whether the cursor is visible
    buffer_mode: Option<BufferMode>,
}

impl CommandBuffer {
    /// Creates a new buffer of size 0. It must be resized later.
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            panel: None,
            cursor_pos: 0,
            buffer_mode: None,
        }
    }

    /// Prints an error overtop of the text in the command buffer.
    pub fn write_error(&mut self, error: &str)
        -> Result<(), TermWriteError>
    {
        if let Some(panel) = &mut self.panel {
            panel.clear();
            panel.write(OverflowMode::Wrap, |writer| {
                writer.write_colored(error, termion::color::LightWhite, termion::color::Red)?;
                Ok(())
            })?;
        };

        // Disable the cursor without re-printing what's in the buffer
        self.buffer_mode = None;

        Ok(())
    }

    /// If [`Some`], returns the mode the buffer is in. [`None`] indicates that
    /// the cursor is invisible.
    pub fn buffer_mode(&self) -> Option<BufferMode> {
        self.buffer_mode
    }

    /// Processes a key event, returning a String if the buffer produced a command.
    pub fn process_key(&mut self, key: Key)
        -> Option<(String, BufferMode)>
    {
        match key {
            Key::Backspace => self.delete_char(),
            // Regaining cursor focus
            Key::Char('>') if self.buffer_mode().is_none() => {
                self.set_buffer_mode(BufferMode::Chat);
            },
            Key::Char('/') if self.buffer_mode().is_none() => {
                self.set_buffer_mode(BufferMode::Command);
            },
            Key::Char('#') if self.buffer_mode().is_none() => {
                self.set_buffer_mode(BufferMode::Admin);
            },
            Key::Char(char) => {
                // Ensure the character is in ASCII range
                match char {
                    '\n' => {
                        return self.flush();
                    },
                    c @ ' '..='~' => {
                        self.insert_char(c).unwrap();
                    },
                    _ => {},
                }
            },
            // Transfer control away from the command buffer
            Key::Esc => {
                self.disable_cursor();
            }
            Key::Left => {{}
                self.move_cursor_left();
            },
            Key::Right => {
                self.move_cursor_right();
            },
            // POSSIBLE USES IN THE FUTURE
            Key::Alt(_) => {},
            Key::Ctrl(_) => {},
            Key::Delete => {},
            Key::Up => {}, // TODO maybe implement recall?
            Key::Down => {},
            // KEYS TO IGNORE
            Key::BackTab => {},
            Key::Insert => {},
            Key::F(_) => {},
            Key::Null => {},
            Key::Home => {},
            Key::End => {},
            Key::PageUp => {},
            Key::PageDown => {},
            _ => {},
        }
        None
    }

    /// Disables the cursor of this command buffer and re-renders it.
    pub fn disable_cursor(&mut self) {
        self.buffer_mode = None;
        self.render();
    }

    pub fn set_buffer_mode(&mut self, mode: BufferMode) {
        self.buffer_mode = Some(mode);
        self.render();
    }

    /// Inserts one character into the buffer at the cursor position and
    /// advances the cursor one to the right. Renders the terminal afterwards.
    /// Fails if the passed char is not permitted by the terminal.
    pub fn insert_char(&mut self, ch: char)
        -> Result<(), TermWriteError>
    {
        TermPanel::test_char(ch)?;

        // Don't allow insert if the buffer is full.
        if self.buffer.len() == self.buffer.capacity() { return Ok(()) }

        // Appending
        if self.cursor_pos == self.buffer.len() {
            self.buffer.push(ch);
        }

        // Inserting
        else {
            self.buffer.insert(self.cursor_pos, ch);
        }
        self.move_cursor_right();

        Ok(())
    }

    /// Removes a char from the buffer. Returns true if a character was
    /// successfully deleted.
    pub fn delete_char(&mut self) {
        if self.cursor_pos == 0 { return }
        if self.cursor_pos == self.buffer.len() {
            self.buffer.pop().unwrap();
        } else {
            self.buffer.remove(self.cursor_pos - 1);
        }

        self.move_cursor_left();
    }

    /// Replaces the character highlighted by the cursor with the desired
    /// character. Returns the rendering instruction that makes this happen.
    // pub fn replace_char(&mut self, ch: char) -> String {
    //     self.buffer[self.cursor_pos] = ch;
    //     // If the cursor is at the end, increase the number of chars
    //     if self.cursor_pos == self.num_chars {
    //         // ...but only if there's space for the cursor to go
    //         if self.num_chars < self.buffer.capacity() - 1 {
    //             self.num_chars += 1;
    //         }
    //     }
    //     self.move_cursor_right();
    //     self.print_buffer() // TODO replace with print_around_cursor
    // }

    /// Moves the cursor left. Returns false if the cursor could not be moved
    /// left.
    pub fn move_cursor_left(&mut self) -> bool {
        if self.cursor_pos == 0 { return false; }
        self.cursor_pos -= 1;
        self.render(); // FIXME: change only one char
        true
    }

    /// Moves the cursor right. Returns false if the cursor could not be moved
    /// left.
    pub fn move_cursor_right(&mut self) -> bool {
        if self.cursor_pos == self.buffer.len() { return false; }
        if self.cursor_pos == self.buffer.capacity() - 1 { return false; }
        self.cursor_pos += 1;
        self.render(); // FIXME: change only one char
        true
    }

    /// Flushes out this buffer and returns its contents. This also returns and
    /// resets this buffer's mode. If there is nothing in the buffer to flush,
    /// this function returns [`None`] and does not modify anything else.
    pub fn flush(&mut self)
        -> Option<(String, BufferMode)>
    { 

        if self.buffer.is_empty() { return None; }

        let command = self.buffer.clone();

        // Reset the object. Now that the buffer is empty, we can properly
        // correct for any resizes that happened while the buffer was full.
        let buffer_size = self.panel.as_ref().map(|panel| panel.dim().area() as usize)
            .unwrap_or(0);
        self.buffer = String::with_capacity(buffer_size);
        self.cursor_pos = 0;

        let buffer_mode = self.buffer_mode.take().unwrap();

        // Rerender the object
        self.render();

        Some((command, buffer_mode))
    }

    pub fn render(&mut self) {

        if let Some(panel) = &mut self.panel {

            panel.clear();
            panel.write(OverflowMode::Wrap, |writer| {

                // Write the buffer symbol
                if let Some(mode) = self.buffer_mode {
                    writer.write_fg_colored(mode.symbol(), mode).unwrap();
                }

                let iter = self.buffer.chars()
                // This will render the cursor at the end of the line
                .chain(std::iter::once(' '))
                .enumerate();

                for (idx, chr) in iter {

                    if idx == self.cursor_pos {
                        if let Some(mode) = self.buffer_mode {
                            writer.write_bg_colored(chr, mode)
                        } else {
                            writer.write_char(chr).map(|_| {})
                        }
                    } else {
                        writer.write_char(chr).map(|_| {})
                    }.unwrap();
                }
            });
        }
    }

    pub fn resize(&mut self, new_panel: TermPanel) {

        // Re-allocate the buffer to have the capacity of the new buffer.
        let mut string = String::with_capacity(new_panel.dim().area() as usize);
        string.clone_from(&self.buffer);
        self.buffer = string;

        self.panel = Some(new_panel);

        self.render();
    }
}

#[cfg(test)]
mod test {
    use crate::client::robust::command_buffer::{CommandBuffer, BufferMode};
    use crate::client::robust::panels::PanelDim;
    use crate::client::robust::terminal::TermPanel;

    #[tokio::test]
    async fn test_buffer() -> std::io::Result<()> {
        let (mut terminal, mut keys) = TermPanel::new()?;
        terminal.reduce_size(PanelDim {
            top_left: (10, 3),
            size: (10, 5),
        });
        let mut buffer = CommandBuffer::new();
        buffer.resize(terminal);
        buffer.set_buffer_mode(BufferMode::Command);
        loop {
            let key = match keys.recv().await {
                Some(v) => v,
                None => break,
            };
            if let Some(msg) = buffer.process_key(key) {
                dbg!(msg);
            }
        }

        Ok(())
    }
}

