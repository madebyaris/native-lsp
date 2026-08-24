//! Open documents. Only the active tab keeps a CST; nap drops parse state.

use std::collections::HashMap;

use crate::cst::{self, GrammarPool, ParserKind};
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
    fn parse(&mut self, intern: &mut Interner, pool: &mut GrammarPool) {
        let old = self.tree.take();
        let out = cst::parse(&self.language_id, &self.text, old.as_ref(), intern, pool);
        self.symbols = Some(out.symbols);
        self.parser = out.kind;
        self.tree = if self.visibility == Visibility::Active {
            out.tree
        } else {
            None
        };
    }

    pub fn nap(&mut self) {
        self.symbols = None;
        self.tree = None;
    }
}

#[derive(Default)]
pub struct Workspace {
    pub intern: Interner,
    docs: HashMap<String, Document>,
    pool: GrammarPool,
}

impl Workspace {
    pub fn open(&mut self, uri: String, language_id: String, text: String) {
        let language_id = lang::normalize_language_id(&language_id, &uri);
        self.demote_active();
        let mut doc = Document {
            uri: uri.clone(),
            language_id,
            text,
            symbols: None,
            tree: None,
            parser: ParserKind::LineScan,
            visibility: Visibility::Active,
        };
        doc.parse(&mut self.intern, &mut self.pool);
        self.docs.insert(uri, doc);
    }

    fn demote_active(&mut self) {
        for doc in self.docs.values_mut() {
            if doc.visibility == Visibility::Active {
                doc.visibility = Visibility::Hidden;
                doc.nap();
            }
        }
    }

    pub fn change(&mut self, uri: &str, text: String) {
        if let Some(doc) = self.docs.get_mut(uri) {
            doc.text = text;
            doc.parse(&mut self.intern, &mut self.pool);
        }
    }

    pub fn close(&mut self, uri: &str) {
        self.docs.remove(uri);
    }

    pub fn get(&self, uri: &str) -> Option<&Document> {
        self.docs.get(uri)
    }

    pub fn ensure_parsed(&mut self, uri: &str) {
        let Some(doc) = self.docs.get_mut(uri) else {
            return;
        };
        if doc.symbols.is_some() {
            return;
        }
        doc.parse(&mut self.intern, &mut self.pool);
    }

    pub fn set_visibility(&mut self, uri: &str, vis: Visibility) {
        if let Some(doc) = self.docs.get_mut(uri) {
            doc.visibility = vis;
            if vis == Visibility::Hidden {
                doc.nap();
            } else if doc.symbols.is_none() || (vis == Visibility::Active && doc.tree.is_none()) {
                doc.parse(&mut self.intern, &mut self.pool);
            } else if vis != Visibility::Active {
                doc.tree = None;
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
        self.pool.clear();
    }

    pub fn wake(&mut self) {
        for doc in self.docs.values_mut() {
            if doc.visibility != Visibility::Hidden && doc.symbols.is_none() {
                doc.parse(&mut self.intern, &mut self.pool);
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

    pub fn grammars_loaded(&self) -> Vec<String> {
        self.pool.loaded()
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
        ws.open("file:///plugin.php".into(), "php".into(), src);
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

    #[test]
    fn only_active_tab_keeps_cst() {
        let mut ws = Workspace::default();
        ws.open(
            "file:///a.php".into(),
            "php".into(),
            "<?php class A { public function boot() {} }\n".into(),
        );
        ws.open(
            "file:///b.py".into(),
            "python".into(),
            "class Store:\n    def checkout(self):\n        pass\n".into(),
        );
        assert_eq!(ws.open_count(), 2);
        assert_eq!(ws.cst_count(), 1, "only the active tab keeps a CST");
        assert_eq!(ws.parsed_count(), 1, "hidden tab is napped");
        assert_eq!(ws.grammars_loaded(), vec!["php".to_string(), "python".to_string()]);
        let py = ws.get("file:///b.py").unwrap();
        assert_eq!(py.visibility, Visibility::Active);
        assert!(py.tree.is_some());
        let php = ws.get("file:///a.php").unwrap();
        assert_eq!(php.visibility, Visibility::Hidden);
        assert!(php.tree.is_none());
        assert!(php.symbols.is_none());

        ws.ensure_parsed("file:///a.php");
        assert!(ws.get("file:///a.php").unwrap().symbols.is_some());
        assert!(
            ws.get("file:///a.php").unwrap().tree.is_none(),
            "hidden hover must not keep a second CST"
        );
        assert_eq!(ws.cst_count(), 1);
    }

    #[test]
    fn park_drops_grammars() {
        let mut ws = Workspace::default();
        ws.open(
            "file:///a.php".into(),
            "php".into(),
            "<?php class A {}\n".into(),
        );
        assert!(!ws.grammars_loaded().is_empty());
        ws.park();
        assert!(ws.grammars_loaded().is_empty());
        assert_eq!(ws.cst_count(), 0);
    }
}
