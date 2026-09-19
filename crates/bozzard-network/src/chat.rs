//! Small, bounded lobby chat. Sender identity comes from Steam, never message text.
use std::collections::VecDeque;

pub const MAX_CHAT_CHARS: usize = 160;
pub const MAX_CHAT_LINES: usize = 4;
pub const CHAT_PREFIX: &[u8] = b"bozzard-chat-v1:";

pub fn clean_text(text: &str, limit: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(limit)
        .collect()
}

#[derive(Clone, Default)]
pub struct ChatLog {
    lines: VecDeque<String>,
}
impl ChatLog {
    pub fn receive(&mut self, name: &str, bytes: &[u8]) -> bool {
        let Some(bytes) = bytes.strip_prefix(CHAT_PREFIX) else {
            return false;
        };
        if bytes.len() > MAX_CHAT_CHARS * 4 {
            return false;
        }
        let Ok(text) = std::str::from_utf8(bytes) else {
            return false;
        };
        let text = clean_text(text, MAX_CHAT_CHARS);
        if text.trim().is_empty() {
            return false;
        }
        self.lines
            .push_back(format!("{}: {}", clean_text(name, 32), text.trim()));
        while self.lines.len() > MAX_CHAT_LINES {
            self.lines.pop_front();
        }
        true
    }
    pub fn text(&self) -> String {
        self.lines.iter().cloned().collect::<Vec<_>>().join("\n")
    }
}
