//! Mixed-language fixture: intern table sketch.

pub enum Kind {
    Class,
    Function,
}

pub struct Index {
    pub interned: u32,
}

impl Index {
    pub fn intern(&mut self, _name: &str) -> u32 {
        self.interned += 1;
        self.interned
    }
}

pub fn lookup(index: &Index, id: u32) -> bool {
    id <= index.interned
}

pub const MAX_OPEN: u32 = 1024;
