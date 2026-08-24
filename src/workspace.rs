//! Open documents. Nap drops parsed symbols; text stays until didClose.

use std::collections::HashMap;

use crate::intern::Interner;
use crate::php::{self, PhpSymbol};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Active,
    Visible,
    Hidden,
}

#[derive(Debug)]
pub struct Document {
    pub uri: String,
    pub text: String,
    pub symbols: Option<Vec<PhpSymbol>>,
    pub visibility: Visibility,
}

impl Document {
    pub fn parse(&mut self, intern: &mut Interner) {
        self.symbols = Some(php::extract(&self.text, intern));
    }

    pub fn nap(&mut self) {
        self.symbols = None;
    }
}

#[derive(Debug, Default)]
pub struct Workspace {
    pub intern: Interner,
    docs: HashMap<String, Document>,
}

impl Workspace {
    pub fn open(&mut self, uri: String, text: String) {
        let mut doc = Document {
            uri: uri.clone(),
            text,
            symbols: None,
            visibility: Visibility::Active,
        };
        doc.parse(&mut self.intern);
        self.docs.insert(uri, doc);
    }

    pub fn change(&mut self, uri: &str, text: String) {
        if let Some(doc) = self.docs.get_mut(uri) {
            doc.text = text;
            doc.parse(&mut self.intern);
        }
    }

    pub fn close(&mut self, uri: &str) {
        self.docs.remove(uri);
    }

    pub fn get(&self, uri: &str) -> Option<&Document> {
        self.docs.get(uri)
    }

    pub fn set_visibility(&mut self, uri: &str, vis: Visibility) {
        if let Some(doc) = self.docs.get_mut(uri) {
            doc.visibility = vis;
            if vis == Visibility::Hidden {
                doc.nap();
            } else if doc.symbols.is_none() {
                doc.parse(&mut self.intern);
            }
        }
    }

    pub fn nap_hidden(&mut self) {
        for doc in self.docs.values_mut() {
            if doc.visibility != Visibility::Active {
                doc.nap();
            }
        }
    }

    pub fn park(&mut self) {
        for doc in self.docs.values_mut() {
            doc.nap();
        }
    }

    pub fn wake(&mut self) {
        for doc in self.docs.values_mut() {
            if doc.visibility != Visibility::Hidden && doc.symbols.is_none() {
                doc.parse(&mut self.intern);
            }
        }
    }

    pub fn open_count(&self) -> usize {
        self.docs.len()
    }

    pub fn parsed_count(&self) -> usize {
        self.docs.values().filter(|d| d.symbols.is_some()).count()
    }

    pub fn all_symbol_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for doc in self.docs.values() {
            if let Some(syms) = &doc.symbols {
                for s in syms {
                    if let Some(n) = self.intern.get(s.name_id) {
                        names.push(n.to_string());
                    }
                    if let Some(id) = s.hook_id {
                        if let Some(n) = self.intern.get(id) {
                            names.push(n.to_string());
                        }
                    }
                }
            }
        }
        for stub in php::WP_STUBS {
            names.push((*stub).to_string());
        }
        names.sort();
        names.dedup();
        names
    }
}
