use super::terminal::{OverflowMode, TermPanelCache, TermPanelUpdate};

#[derive(Debug)]
pub struct ChatPanel {
    panel: TermPanelCache,
    buffer: Vec<Box<str>>,
}

impl ChatPanel {
    /// Constructs a new [`ChatPanel`] with no panel. To begin rendering, call
    /// the [`resize`] function.
    pub fn new(panel: TermPanelUpdate) -> Self {
        Self {
            panel: panel.into(),
            buffer: Vec::new(),
        }
    }

    /// Adds a message to the chat panel and re-renders the panel.
    pub fn add_message(&mut self, msg: Box<str>) {
        self.buffer.push(msg);
        self.render();
    }

    fn render(&mut self) {
        self.panel.clear();
        self.panel.write(OverflowMode::Wrap, |writer| {
            for msg in self.buffer.iter().rev() {
                writer.write_str(&*msg).unwrap();
                writer.new_line();
            }
        });
    }

    pub fn resize(&mut self, new_panel: TermPanelUpdate) {
        self.panel.update(new_panel);
        self.render();
    }
}
