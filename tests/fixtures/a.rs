//! Fixture module.

/// Doc comment.
pub fn top_level(x: u64) -> u64 {
    x
}

pub struct Widget {
    pub id: u64,
}

impl Widget {
    pub fn new(id: u64) -> Self {
        Self { id }
    }
    fn hidden(&self) -> u64 {
        self.id
    }
}

mod inner {
    pub fn nested() {}
}
