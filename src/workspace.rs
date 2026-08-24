//! Open documents. Nap drops parsed symbols and the CST; text stays until didClose.

use std::collections::HashMap;

use crate::cst::{self, ParserKind};
use crate::intern::Interner;
use crate::lang;
use crate::php;
use crate::symbol::Symbol;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Active,
    Visible,
    Hidden,
}

#[derive(Debug)]
pub struct Document {
    pub uri: String,
    pub language_id: String,
    pub text: String,
    pub symbols: Option<Vec<Symbol>>,
    pub tree: Option<tree_sitter::Tree>,
    pub parser: ParserKind,
    pub visibility: Visibility,
}

impl Document {
    pub fn parse(&mut self, intern: &mut Interner) {
        let old = self.tree.take();
        let out = cst::parse(&self.language_id, &self.text, old.as_ref(), intern);
        self.symbols = Some(out.symbols);
        self.tree = out.tree;
        self.parser = out.kind;
    }

    pub fn nap(&mut self) {
        self.symbols = None;
        self.tree = None;
    }
}

#[derive(Debug, Default)]
pub struct Workspace {
    pub intern: Interner,
    docs: HashMap<String, Document>,
}

impl Workspace {
    pub fn open(&mut self, uri: String, language_id: String, text: String) {
        let language_id = lang::normalize_language_id(&language_id, &uri);
        let mut doc = Document {
            uri: uri.clone(),
            language_id,
            text,
            symbols: None,
            tree: None,
            parser: ParserKind::LineScan,
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

    pub fn cst_count(&self) -> usize {
        self.docs.values().filter(|d| d.tree.is_some()).count()
    }

    pub fn parser_kinds(&self) -> Vec<String> {
        let mut kinds: Vec<String> = self
            .docs
            .values()
            .filter(|d| d.symbols.is_some())
            .map(|d| d.parser.label().to_string())
            .collect();
        kinds.sort();
        kinds.dedup();
        kinds
    }

    pub fn language_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.docs.values().map(|d| d.language_id.clone()).collect();
        ids.sort();
        ids.dedup();
        ids
    }

    pub fn has_php(&self) -> bool {
        self.docs.values().any(|d| d.language_id == "php")
    }

    pub fn all_symbol_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for doc in self.docs.values() {
            if let Some(syms) = &doc.symbols {
                for s in syms {
                    if let Some(n) = self.intern.get(s.name_id) {
                        names.push(n.to_string());
                    }
                    if let Some(id) = s.extra_id {
                        if let Some(n) = self.intern.get(id) {
                            names.push(n.to_string());
                        }
                    }
                }
            }
        }
        if self.has_php() {
            for stub in php::WP_STUBS {
                names.push((*stub).to_string());
            }
        }
        names.sort();
        names.dedup();
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn php_open_keeps_cst_until_nap() {
        let mut ws = Workspace::default();
        let src = std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("testdata/mixed/01-plugin.php"),
        )
        .unwrap();
        ws.open(
            "file:///plugin.php".into(),
            "php".into(),
            src,
        );
        assert_eq!(ws.parsed_count(), 1);
        assert_eq!(ws.cst_count(), 1);
        assert_eq!(ws.parser_kinds(), vec!["tree-sitter".to_string()]);
        ws.set_visibility("file:///plugin.php", Visibility::Hidden);
        assert_eq!(ws.parsed_count(), 0);
        assert_eq!(ws.cst_count(), 0);
        ws.set_visibility("file:///plugin.php", Visibility::Active);
        assert_eq!(ws.parsed_count(), 1);
        assert_eq!(ws.cst_count(), 1);
    }
}
