//! Open-file tree-sitter CST. Nap drops the tree; closed files are not kept.

use tree_sitter::{Node, Parser, Tree};

use crate::intern::Interner;
use crate::lang;
use crate::symbol::{push_ident, Symbol, SymbolKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParserKind {
    TreeSitter,
    LineScan,
}

impl ParserKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::TreeSitter => "tree-sitter",
            Self::LineScan => "line-scan",
        }
    }
}

pub struct ParseOutcome {
    pub symbols: Vec<Symbol>,
    pub tree: Option<Tree>,
    pub kind: ParserKind,
}

pub fn parse(
    language_id: &str,
    text: &str,
    old_tree: Option<&Tree>,
    intern: &mut Interner,
) -> ParseOutcome {
    if language_id == "php" {
        if let Some(out) = parse_php(text, old_tree, intern) {
            return out;
        }
    }
    ParseOutcome {
        symbols: lang::extract(language_id, text, intern),
        tree: None,
        kind: ParserKind::LineScan,
    }
}

fn parse_php(text: &str, old_tree: Option<&Tree>, intern: &mut Interner) -> Option<ParseOutcome> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_php::language_php()).ok()?;
    let tree = parser.parse(text, old_tree)?;
    let symbols = extract_php_tree(&tree, text.as_bytes(), intern);
    Some(ParseOutcome {
        symbols,
        tree: Some(tree),
        kind: ParserKind::TreeSitter,
    })
}

fn extract_php_tree(tree: &Tree, src: &[u8], intern: &mut Interner) -> Vec<Symbol> {
    let mut out = Vec::new();
    walk(tree.root_node(), src, intern, &mut out);
    out
}

fn walk(node: Node, src: &[u8], intern: &mut Interner, out: &mut Vec<Symbol>) {
    match node.kind() {
        "class_declaration" | "interface_declaration" | "trait_declaration" | "enum_declaration" => {
            named_symbol(node, src, intern, out, SymbolKind::Class);
        }
        "function_definition" | "method_declaration" => {
            named_symbol(node, src, intern, out, SymbolKind::Function);
        }
        "function_call_expression" => {
            php_hook(node, src, intern, out);
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, intern, out);
    }
}

fn named_symbol(
    node: Node,
    src: &[u8],
    intern: &mut Interner,
    out: &mut Vec<Symbol>,
    kind: SymbolKind,
) {
    let Some(name) = node.child_by_field_name("name") else {
        return;
    };
    let Ok(text) = name.utf8_text(src) else {
        return;
    };
    if text.is_empty() {
        return;
    }
    let pos = name.start_position();
    push_ident(
        out,
        intern,
        kind,
        text,
        pos.row as u32,
        pos.column as u32,
        None,
    );
}

fn php_hook(node: Node, src: &[u8], intern: &mut Interner, out: &mut Vec<Symbol>) {
    let Some(func) = node.child_by_field_name("function") else {
        return;
    };
    let fname = leaf_name(func, src);
    if fname != "add_action" && fname != "add_filter" {
        return;
    }
    let Some(args) = node.child_by_field_name("arguments") else {
        return;
    };
    let Some(hook) = first_string_arg(args, src) else {
        return;
    };
    let pos = node.start_position();
    push_ident(
        out,
        intern,
        SymbolKind::Hook,
        fname,
        pos.row as u32,
        pos.column as u32,
        Some(&hook),
    );
}

fn leaf_name<'a>(node: Node<'a>, src: &'a [u8]) -> &'a str {
    if node.kind() == "qualified_name" {
        let mut cursor = node.walk();
        let mut last = "";
        for child in node.children(&mut cursor) {
            if child.kind() == "name" {
                if let Ok(t) = child.utf8_text(src) {
                    last = t;
                }
            }
        }
        return last;
    }
    node.utf8_text(src).unwrap_or("")
}

fn first_string_arg<'a>(args: Node<'a>, src: &'a [u8]) -> Option<&'a str> {
    let mut cursor = args.walk();
    for child in args.children(&mut cursor) {
        if is_php_string(child.kind()) {
            return Some(unquote(child.utf8_text(src).ok()?));
        }
        if child.kind() != "argument" {
            continue;
        }
        let mut inner = child.walk();
        for n in child.children(&mut inner) {
            if is_php_string(n.kind()) {
                return Some(unquote(n.utf8_text(src).ok()?));
            }
        }
    }
    None
}

fn is_php_string(kind: &str) -> bool {
    matches!(kind, "string" | "encapsed_string" | "heredoc" | "nowdoc")
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.len() >= 2 {
        let a = bytes[0];
        let b = bytes[bytes.len() - 1];
        if (a == b && (a == b'"' || a == b'\'')) || (a == b'b' && bytes.len() >= 3) {
            if a == b'"' || a == b'\'' {
                return &s[1..s.len() - 1];
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(text: &str) -> (Vec<String>, ParserKind, bool) {
        let mut intern = Interner::new();
        let out = parse("php", text, None, &mut intern);
        let names = out
            .symbols
            .iter()
            .map(|s| intern.get(s.name_id).unwrap().to_string())
            .collect();
        (names, out.kind, out.tree.is_some())
    }

    #[test]
    fn php_tree_sitter_finds_class_method_and_hooks() {
        let src = std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/mixed/01-plugin.php"),
        )
        .unwrap();
        let (n, kind, has_tree) = names(&src);
        assert_eq!(kind, ParserKind::TreeSitter);
        assert!(has_tree);
        assert!(n.contains(&"Mixed_Plugin".into()));
        assert!(n.contains(&"boot".into()));
        assert!(n.contains(&"filter_content".into()));
        assert!(n.contains(&"mixed_plugin_helper".into()));
        assert!(n.contains(&"add_action".into()));
        assert!(n.contains(&"add_filter".into()));
    }

    #[test]
    fn tree_sitter_sees_same_line_method_line_scan_misses() {
        let src = "<?php class Foo { public function bar() {} }\n";
        let mut intern = Interner::new();
        let line = crate::php::extract(src, &mut intern);
        assert!(
            line.is_empty(),
            "line-scan should miss a class and method on the same line after <?php"
        );

        let (n, kind, _) = names(src);
        assert_eq!(kind, ParserKind::TreeSitter);
        assert!(n.contains(&"Foo".into()));
        assert!(n.contains(&"bar".into()));
    }

    #[test]
    fn other_languages_stay_line_scan() {
        let mut intern = Interner::new();
        let out = parse("python", "class Store:\n    def checkout(self):\n        pass\n", None, &mut intern);
        assert_eq!(out.kind, ParserKind::LineScan);
        assert!(out.tree.is_none());
        let n: Vec<_> = out
            .symbols
            .iter()
            .map(|s| intern.get(s.name_id).unwrap())
            .collect();
        assert!(n.contains(&"Store"));
        assert!(n.contains(&"checkout"));
    }
}
