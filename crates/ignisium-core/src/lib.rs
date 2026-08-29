#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextOffset(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRange {
    pub start: TextOffset,
    pub end: TextOffset,
}

impl TextRange {
    pub fn new(start: TextOffset, end: TextOffset) -> Self {
        Self { start, end }
    }

    pub fn len(&self) -> usize {
        self.end.0 - self.start.0
    }
}

pub struct Document {
    text: String,
}

impl Document {
    pub fn new() -> Self {
        Self {
            text: String::new(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn insert(&mut self, offset: TextOffset, text: &str) {
        self.text.insert_str(offset.0, text);
    }

    pub fn delete(&mut self, range: TextRange) {
        self.text.replace_range(range.start.0..range.end.0, "");
    }
}
