use super::terminal::{TermPanel, OverflowMode};

#[derive(Debug)]
pub(super) struct ChatPanel {
    panel: Option<TermPanel>,
    buffer: Vec<Box<str>>,
}

impl ChatPanel {
    /// Constructs a new [`ChatPanel`] with no panel. To begin rendering, call
    /// the [`resize`] function.
    pub fn new() -> Self {
        Self {
            panel: None,
            buffer: Vec::new(),
        }
    }

    /// Adds a message to the chat panel and re-renders the panel.
    pub fn add_message(&mut self, msg: Box<str>) {
        self.buffer.push(msg);
        self.render();
    }

    pub fn render(&mut self) {

        if let Some(panel) = &mut self.panel {
            panel.clear();
            panel.write(OverflowMode::Wrap, |writer| {
                for msg in self.buffer.iter().rev() {
                    writer.write_str(&*msg).unwrap();
                    writer.new_line();
                }
            });
        }
    }

    pub fn resize(&mut self, new_panel: TermPanel) {
        self.panel = Some(new_panel);
        self.render();
    }
}
