//! Open-file tree-sitter CST. Grammars load lazily; nap drops the tree.

use std::collections::HashMap;

use tree_sitter::{Language, Node, Parser, Tree};

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

#[derive(Default)]
pub struct GrammarPool {
    parsers: HashMap<String, Parser>,
}

impl GrammarPool {
    pub fn loaded(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.parsers.keys().cloned().collect();
        ids.sort();
        ids
    }

    pub fn clear(&mut self) {
        self.parsers.clear();
    }

    fn parse_tree(&mut self, language_id: &str, text: &str, old: Option<&Tree>) -> Option<Tree> {
        if !self.parsers.contains_key(language_id) {
            let lang = grammar(language_id)?;
            let mut parser = Parser::new();
            parser.set_language(&lang).ok()?;
            self.parsers.insert(language_id.to_string(), parser);
        }
        self.parsers.get_mut(language_id)?.parse(text.as_bytes(), old)
    }
}

pub fn has_grammar(language_id: &str) -> bool {
    grammar(language_id).is_some()
}

fn grammar(language_id: &str) -> Option<Language> {
    Some(match language_id {
        "php" => tree_sitter_php::language_php(),
        "javascript" => tree_sitter_javascript::language(),
        "typescript" => tree_sitter_typescript::language_typescript(),
        "html" => tree_sitter_html::language(),
        "css" => tree_sitter_css::language(),
        "json" => tree_sitter_json::language(),
        "yaml" => tree_sitter_yaml::language(),
        "python" => tree_sitter_python::language(),
        "rust" => tree_sitter_rust::language(),
        _ => return None,
    })
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
    pool: &mut GrammarPool,
) -> ParseOutcome {
    if let Some(out) = parse_grammar(language_id, text, old_tree, intern, pool) {
        return out;
    }
    ParseOutcome {
        symbols: lang::extract(language_id, text, intern),
        tree: None,
        kind: ParserKind::LineScan,
    }
}

fn parse_grammar(
    language_id: &str,
    text: &str,
    old_tree: Option<&Tree>,
    intern: &mut Interner,
    pool: &mut GrammarPool,
) -> Option<ParseOutcome> {
    let tree = pool.parse_tree(language_id, text, old_tree)?;
    let symbols = extract_tree(language_id, &tree, text.as_bytes(), intern);
    Some(ParseOutcome {
        symbols,
        tree: Some(tree),
        kind: ParserKind::TreeSitter,
    })
}

fn extract_tree(language_id: &str, tree: &Tree, src: &[u8], intern: &mut Interner) -> Vec<Symbol> {
    let mut out = Vec::new();
    match language_id {
        "php" => walk_php(tree.root_node(), src, intern, &mut out),
        "javascript" => walk_js(tree.root_node(), src, intern, &mut out, false),
        "typescript" => walk_js(tree.root_node(), src, intern, &mut out, true),
        "html" => walk_html(tree.root_node(), src, intern, &mut out),
        "css" => walk_css(tree.root_node(), src, intern, &mut out),
        "json" => walk_json(tree.root_node(), src, intern, &mut out),
        "yaml" => walk_yaml(tree.root_node(), src, intern, &mut out),
        "python" => walk_python(tree.root_node(), src, intern, &mut out),
        "rust" => walk_rust(tree.root_node(), src, intern, &mut out),
        _ => {}
    }
    out
}

fn walk_named(
    node: Node,
    src: &[u8],
    intern: &mut Interner,
    out: &mut Vec<Symbol>,
    kinds: &[(&str, SymbolKind)],
) {
    if let Some((_, kind)) = kinds.iter().find(|(k, _)| node.kind() == *k) {
        named_symbol(node, src, intern, out, *kind);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_named(child, src, intern, out, kinds);
    }
}

fn named_symbol(
    node: Node,
    src: &[u8],
    intern: &mut Interner,
    out: &mut Vec<Symbol>,
    kind: SymbolKind,
) {
    let Some(text) = field_text(node, src, "name").or_else(|| field_text(node, src, "type")) else {
        return;
    };
    if !is_ident(text) {
        return;
    }
    let pos = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("type"))
        .unwrap_or(node)
        .start_position();
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

fn field_text<'a>(node: Node<'a>, src: &'a [u8], field: &str) -> Option<&'a str> {
    let child = node.child_by_field_name(field)?;
    let text = child.utf8_text(src).ok()?;
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_' || first == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '\\')
}

fn walk_php(node: Node, src: &[u8], intern: &mut Interner, out: &mut Vec<Symbol>) {
    match node.kind() {
        "class_declaration" | "interface_declaration" | "trait_declaration" | "enum_declaration" => {
            named_symbol(node, src, intern, out, SymbolKind::Class);
        }
        "function_definition" | "method_declaration" => {
            named_symbol(node, src, intern, out, SymbolKind::Function);
        }
        "function_call_expression" => php_hook(node, src, intern, out),
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_php(child, src, intern, out);
    }
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
        Some(hook),
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
        if a == b && (a == b'"' || a == b'\'') {
            return &s[1..s.len() - 1];
        }
    }
    s
}

fn walk_js(node: Node, src: &[u8], intern: &mut Interner, out: &mut Vec<Symbol>, typescript: bool) {
    match node.kind() {
        "class_declaration" | "class" => {
            named_symbol(node, src, intern, out, SymbolKind::Class);
        }
        "function_declaration" | "generator_function_declaration" | "method_definition" => {
            named_symbol(node, src, intern, out, SymbolKind::Function);
        }
        "interface_declaration" | "type_alias_declaration" | "enum_declaration"
            if typescript =>
        {
            named_symbol(node, src, intern, out, SymbolKind::Type);
        }
        "variable_declarator" => {
            if js_is_fn_value(node) {
                named_symbol(node, src, intern, out, SymbolKind::Function);
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_js(child, src, intern, out, typescript);
    }
}

fn js_is_fn_value(node: Node) -> bool {
    let Some(value) = node.child_by_field_name("value") else {
        return false;
    };
    matches!(
        value.kind(),
        "arrow_function" | "function" | "generator_function"
    )
}

fn walk_html(node: Node, src: &[u8], intern: &mut Interner, out: &mut Vec<Symbol>) {
    if node.kind() == "tag_name" {
        if let Ok(name) = node.utf8_text(src) {
            if name.contains('-') {
                let pos = node.start_position();
                push_ident(
                    out,
                    intern,
                    SymbolKind::Tag,
                    name,
                    pos.row as u32,
                    pos.column as u32,
                    None,
                );
            }
        }
    }
    if node.kind() == "attribute" {
        if let (Some(name), Some(value)) = (
            field_text(node, src, "name").or_else(|| html_attr_name(node, src)),
            html_attr_value(node, src),
        ) {
            let pos = node.start_position();
            if name == "id" {
                push_ident(
                    out,
                    intern,
                    SymbolKind::Tag,
                    value,
                    pos.row as u32,
                    pos.column as u32,
                    Some("id"),
                );
            } else if name == "class" {
                for class in value.split_whitespace() {
                    if !class.is_empty() {
                        push_ident(
                            out,
                            intern,
                            SymbolKind::Rule,
                            class,
                            pos.row as u32,
                            pos.column as u32,
                            Some("class"),
                        );
                    }
                }
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_html(child, src, intern, out);
    }
}

fn html_attr_name<'a>(node: Node<'a>, src: &'a [u8]) -> Option<&'a str> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "attribute_name" {
            return child.utf8_text(src).ok();
        }
    }
    None
}

fn html_attr_value<'a>(node: Node<'a>, src: &'a [u8]) -> Option<&'a str> {
    if let Some(v) = field_text(node, src, "value") {
        return Some(unquote(v));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "quoted_attribute_value" || child.kind() == "attribute_value" {
            return Some(unquote(child.utf8_text(src).ok()?));
        }
    }
    None
}

fn walk_css(node: Node, src: &[u8], intern: &mut Interner, out: &mut Vec<Symbol>) {
    match node.kind() {
        "class_selector" | "id_selector" => {
            if let Some(name) = css_name(node, src) {
                let pos = node.start_position();
                push_ident(
                    out,
                    intern,
                    SymbolKind::Rule,
                    name,
                    pos.row as u32,
                    pos.column as u32,
                    None,
                );
            }
        }
        "keyframes_statement" => {
            if let Some(name) = field_text(node, src, "name").or_else(|| css_name(node, src)) {
                let pos = node.start_position();
                push_ident(
                    out,
                    intern,
                    SymbolKind::Rule,
                    name,
                    pos.row as u32,
                    pos.column as u32,
                    Some("@keyframes"),
                );
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_css(child, src, intern, out);
    }
}

fn css_name<'a>(node: Node<'a>, src: &'a [u8]) -> Option<&'a str> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(
            child.kind(),
            "class_name" | "id_name" | "keyframes_name" | "identifier"
        ) {
            return child.utf8_text(src).ok();
        }
    }
    None
}

fn walk_json(node: Node, src: &[u8], intern: &mut Interner, out: &mut Vec<Symbol>) {
    if node.kind() == "pair" {
        if let Some(key) = field_text(node, src, "key") {
            let key = unquote(key);
            if !key.is_empty() {
                let pos = node.start_position();
                push_ident(
                    out,
                    intern,
                    SymbolKind::Key,
                    key,
                    pos.row as u32,
                    pos.column as u32,
                    None,
                );
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_json(child, src, intern, out);
    }
}

fn walk_yaml(node: Node, src: &[u8], intern: &mut Interner, out: &mut Vec<Symbol>) {
    if node.kind() == "block_mapping_pair" || node.kind() == "flow_pair" {
        if let Some(key) = field_text(node, src, "key") {
            let key = key.trim();
            if is_ident(key) {
                let pos = node.start_position();
                push_ident(
                    out,
                    intern,
                    SymbolKind::Key,
                    key,
                    pos.row as u32,
                    pos.column as u32,
                    None,
                );
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_yaml(child, src, intern, out);
    }
}

fn walk_python(node: Node, src: &[u8], intern: &mut Interner, out: &mut Vec<Symbol>) {
    walk_named(
        node,
        src,
        intern,
        out,
        &[
            ("class_definition", SymbolKind::Class),
            ("function_definition", SymbolKind::Function),
        ],
    );
}

fn walk_rust(node: Node, src: &[u8], intern: &mut Interner, out: &mut Vec<Symbol>) {
    walk_named(
        node,
        src,
        intern,
        out,
        &[
            ("function_item", SymbolKind::Function),
            ("struct_item", SymbolKind::Class),
            ("enum_item", SymbolKind::Type),
            ("trait_item", SymbolKind::Type),
            ("mod_item", SymbolKind::Module),
            ("const_item", SymbolKind::Variable),
            ("type_item", SymbolKind::Type),
            ("impl_item", SymbolKind::Class),
            ("macro_definition", SymbolKind::Function),
        ],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_lang(lang: &str, text: &str) -> (Vec<String>, ParserKind, bool) {
        let mut intern = Interner::new();
        let mut pool = GrammarPool::default();
        let out = parse(lang, text, None, &mut intern, &mut pool);
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
        let (n, kind, has_tree) = parse_lang("php", &src);
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
        assert!(line.is_empty());
        let (n, kind, _) = parse_lang("php", src);
        assert_eq!(kind, ParserKind::TreeSitter);
        assert!(n.contains(&"Foo".into()));
        assert!(n.contains(&"bar".into()));
    }

    #[test]
    fn mixed_fixtures_use_tree_sitter_except_sql() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/mixed");
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.is_file())
            .collect();
        files.sort();
        let mut pool = GrammarPool::default();
        for path in files {
            let text = std::fs::read_to_string(&path).unwrap();
            let lang = crate::lang::infer_from_uri(&format!("file://{}", path.display()));
            let mut intern = Interner::new();
            let out = parse(&lang, &text, None, &mut intern, &mut pool);
            assert!(
                !out.symbols.is_empty(),
                "{} ({lang}) produced no symbols",
                path.display()
            );
            if lang == "sql" {
                assert_eq!(out.kind, ParserKind::LineScan);
                assert!(out.tree.is_none());
            } else {
                assert_eq!(out.kind, ParserKind::TreeSitter, "{lang}");
                assert!(out.tree.is_some(), "{lang}");
            }
        }
        assert_eq!(
            pool.loaded(),
            vec![
                "css",
                "html",
                "javascript",
                "json",
                "php",
                "python",
                "rust",
                "typescript",
                "yaml",
            ]
        );
    }

    #[test]
    fn sql_stays_line_scan() {
        let (n, kind, tree) = parse_lang("sql", "CREATE TABLE posts (id INT);\n");
        assert_eq!(kind, ParserKind::LineScan);
        assert!(!tree);
        assert!(n.contains(&"posts".into()));
    }
}
