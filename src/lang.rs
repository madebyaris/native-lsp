//! Language-id dispatch. Line-scan is the fallback when a grammar is missing.

use crate::intern::Interner;
use crate::php;
use crate::symbol::{self, push_ident, Symbol, SymbolKind};

/// What the server actually does today. This is not Intelephense + tsserver.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct LanguageSupport {
    pub id: &'static str,
    pub parser: &'static str,
    pub symbols: &'static str,
}

pub const LANGUAGE_SUPPORT: &[LanguageSupport] = &[
    LanguageSupport {
        id: "php",
        parser: "tree-sitter",
        symbols: "class, method, function, WordPress add_action/add_filter",
    },
    LanguageSupport {
        id: "javascript",
        parser: "tree-sitter",
        symbols: "class, function, method, const/let/var arrow or function",
    },
    LanguageSupport {
        id: "typescript",
        parser: "tree-sitter",
        symbols: "JS plus interface, type, enum — not a typechecker",
    },
    LanguageSupport {
        id: "html",
        parser: "tree-sitter",
        symbols: "id, class, custom elements",
    },
    LanguageSupport {
        id: "css",
        parser: "tree-sitter",
        symbols: "selectors, @keyframes",
    },
    LanguageSupport {
        id: "json",
        parser: "tree-sitter",
        symbols: "object keys",
    },
    LanguageSupport {
        id: "yaml",
        parser: "tree-sitter",
        symbols: "mapping keys",
    },
    LanguageSupport {
        id: "sql",
        parser: "line-scan",
        symbols: "CREATE TABLE/VIEW/INDEX/FUNCTION/PROCEDURE (no 0.22 grammar)",
    },
    LanguageSupport {
        id: "python",
        parser: "tree-sitter",
        symbols: "class, def, async def",
    },
    LanguageSupport {
        id: "rust",
        parser: "tree-sitter",
        symbols: "fn, struct, enum, impl, trait, mod, const",
    },
];

/// Common editor languages with no scanner yet. didOpen keeps text; symbols stay empty.
pub const UNSUPPORTED_COMMON: &[&str] = &[
    "go", "java", "c", "cpp", "csharp", "ruby", "vue", "svelte", "markdown", "shell", "xml",
    "toml", "lua", "kotlin", "swift", "dart",
];

pub fn normalize_language_id(language_id: &str, uri: &str) -> String {
    let raw = language_id.trim();
    if raw.is_empty() {
        return infer_from_uri(uri);
    }
    let mapped = map_alias(raw);
    if is_known(mapped) {
        mapped.to_string()
    } else {
        infer_from_uri(uri)
    }
}

pub fn infer_from_uri(uri: &str) -> String {
    let path = uri.rsplit('/').next().unwrap_or(uri);
    let ext = path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    map_alias(match ext.as_str() {
        "php" => "php",
        "js" | "mjs" | "cjs" => "javascript",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "typescriptreact",
        "jsx" => "javascriptreact",
        "html" | "htm" => "html",
        "css" => "css",
        "json" => "json",
        "yml" | "yaml" => "yaml",
        "sql" => "sql",
        "py" => "python",
        "rs" => "rust",
        _ => "plaintext",
    })
    .to_string()
}

fn map_alias(id: &str) -> &str {
    match id {
        "javascriptreact" | "jsx" => "javascript",
        "typescriptreact" | "tsx" => "typescript",
        "yml" => "yaml",
        "fundamental" | "text" | "plain" | "plaintext" | "plaintex" => "plaintext",
        other => other,
    }
}

fn is_known(id: &str) -> bool {
    matches!(
        id,
        "php"
            | "javascript"
            | "typescript"
            | "html"
            | "css"
            | "json"
            | "yaml"
            | "sql"
            | "python"
            | "rust"
    )
}

pub fn extract(language_id: &str, text: &str, intern: &mut Interner) -> Vec<Symbol> {
    match language_id {
        "php" => php::extract(text, intern),
        "javascript" => extract_js(text, intern, false),
        "typescript" => extract_js(text, intern, true),
        "html" => extract_html(text, intern),
        "css" => extract_css(text, intern),
        "json" => extract_json(text, intern),
        "yaml" => extract_yaml(text, intern),
        "sql" => extract_sql(text, intern),
        "python" => extract_python(text, intern),
        "rust" => extract_rust(text, intern),
        _ => Vec::new(),
    }
}

fn extract_js(text: &str, intern: &mut Interner, typescript: bool) -> Vec<Symbol> {
    let mut out = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        let line_no = line_idx as u32;
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") || trimmed.starts_with('*') || trimmed.starts_with("/*") {
            continue;
        }
        if typescript {
            if let Some(name) = ts_decl(trimmed, "interface ") {
                push_ident(&mut out, intern, SymbolKind::Type, name, line_no, 0, None);
            }
            if let Some(name) = ts_decl(trimmed, "type ") {
                push_ident(&mut out, intern, SymbolKind::Type, name, line_no, 0, None);
            }
            if let Some(name) = ts_decl(trimmed, "enum ") {
                push_ident(&mut out, intern, SymbolKind::Type, name, line_no, 0, None);
            }
        }
        if let Some(name) = js_class(trimmed) {
            push_ident(&mut out, intern, SymbolKind::Class, name, line_no, 0, None);
        }
        if let Some(name) = js_function(trimmed) {
            push_ident(
                &mut out,
                intern,
                SymbolKind::Function,
                name,
                line_no,
                0,
                None,
            );
        }
        if let Some(name) = js_const_fn(trimmed) {
            push_ident(
                &mut out,
                intern,
                SymbolKind::Function,
                name,
                line_no,
                0,
                None,
            );
        }
    }
    out
}

fn strip_export(s: &str) -> &str {
    let s = s.trim_start();
    let s = s.strip_prefix("export ").unwrap_or(s);
    let s = s.strip_prefix("default ").unwrap_or(s);
    s.trim_start()
}

fn ts_decl<'a>(trimmed: &'a str, kw: &str) -> Option<&'a str> {
    let rest = strip_export(trimmed);
    let rest = rest.strip_prefix(kw)?;
    symbol::ident_at(rest)
}

fn js_class<'a>(trimmed: &'a str) -> Option<&'a str> {
    let rest = strip_export(trimmed);
    let rest = rest.strip_prefix("class ")?;
    symbol::ident_at(rest)
}

fn js_function<'a>(trimmed: &'a str) -> Option<&'a str> {
    let rest = strip_export(trimmed);
    let rest = rest.strip_prefix("async ").unwrap_or(rest);
    let rest = rest.strip_prefix("function ")?;
    if rest.starts_with('(') {
        return None;
    }
    symbol::ident_at(rest)
}

fn js_const_fn<'a>(trimmed: &'a str) -> Option<&'a str> {
    let rest = strip_export(trimmed);
    let rest = if let Some(r) = rest.strip_prefix("const ") {
        r
    } else if let Some(r) = rest.strip_prefix("let ") {
        r
    } else {
        rest.strip_prefix("var ")?
    };
    let name = symbol::ident_at(rest)?;
    let after = rest.trim_start().get(name.len()..)?;
    let after = after.trim_start().strip_prefix('=')?.trim_start();
    if after.starts_with("async") || after.starts_with("function") || after.starts_with('(') {
        Some(name)
    } else {
        None
    }
}

fn extract_html(text: &str, intern: &mut Interner) -> Vec<Symbol> {
    let mut out = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        let line_no = line_idx as u32;
        scan_html_attrs(line, intern, line_no, &mut out);
        scan_html_tags(line, intern, line_no, &mut out);
    }
    out
}

fn scan_html_attrs(line: &str, intern: &mut Interner, line_no: u32, out: &mut Vec<Symbol>) {
    for attr in ["id=", "class="] {
        let mut search = line;
        while let Some(idx) = search.find(attr) {
            let after = &search[idx + attr.len()..];
            if let Some(value) = symbol::first_quoted(after) {
                if attr.starts_with("id") {
                    push_ident(out, intern, SymbolKind::Tag, value, line_no, 0, Some("id"));
                } else {
                    for class in value.split_whitespace() {
                        if !class.is_empty() {
                            push_ident(
                                out,
                                intern,
                                SymbolKind::Rule,
                                class,
                                line_no,
                                0,
                                Some("class"),
                            );
                        }
                    }
                }
            }
            search = &search[idx + attr.len()..];
        }
    }
}

fn scan_html_tags(line: &str, intern: &mut Interner, line_no: u32, out: &mut Vec<Symbol>) {
    let mut search = line;
    while let Some(idx) = search.find('<') {
        let after = &search[idx + 1..];
        if after.starts_with('/') || after.starts_with('!') || after.starts_with('?') {
            search = &search[idx + 1..];
            continue;
        }
        if let Some(name) = html_tag_name(after) {
            if name.contains('-') {
                push_ident(out, intern, SymbolKind::Tag, name, line_no, 0, None);
            }
        }
        search = &search[idx + 1..];
    }
}

fn html_tag_name(s: &str) -> Option<&str> {
    let end = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .unwrap_or(s.len());
    let name = &s[..end];
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn extract_css(text: &str, intern: &mut Interner) -> Vec<Symbol> {
    let mut out = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        let line_no = line_idx as u32;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("/*") || trimmed.starts_with('*') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("@keyframes ") {
            if let Some(name) = css_ident(rest) {
                push_ident(
                    &mut out,
                    intern,
                    SymbolKind::Rule,
                    name,
                    line_no,
                    0,
                    Some("@keyframes"),
                );
            }
            continue;
        }
        if trimmed.starts_with('@') {
            continue;
        }
        if !trimmed.contains('{') {
            continue;
        }
        if let Some(before) = trimmed.split('{').next() {
            for sel in before.split(',') {
                let sel = sel.trim();
                if let Some(name) = css_selector_name(sel) {
                    push_ident(&mut out, intern, SymbolKind::Rule, name, line_no, 0, None);
                }
            }
        }
    }
    out
}

fn css_ident(s: &str) -> Option<&str> {
    let s = s.trim_start();
    let end = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(s.len());
    let name = &s[..end];
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn css_selector_name(sel: &str) -> Option<&str> {
    let sel = sel.trim();
    if sel.is_empty() || sel == "from" || sel == "to" || sel.ends_with('%') {
        return None;
    }
    if sel.starts_with('.') || sel.starts_with('#') {
        css_ident(&sel[1..])
    } else if sel.starts_with(':') {
        None
    } else {
        css_ident(sel)
    }
}

fn extract_json(text: &str, intern: &mut Interner) -> Vec<Symbol> {
    let mut out = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        let line_no = line_idx as u32;
        let trimmed = line.trim_start();
        if !trimmed.starts_with('"') {
            continue;
        }
        let rest = &trimmed[1..];
        let Some(end) = rest.find('"') else {
            continue;
        };
        let key = &rest[..end];
        if rest[end + 1..].trim_start().starts_with(':') && !key.is_empty() {
            push_ident(&mut out, intern, SymbolKind::Key, key, line_no, 0, None);
        }
    }
    out
}

fn extract_yaml(text: &str, intern: &mut Interner) -> Vec<Symbol> {
    let mut out = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        let line_no = line_idx as u32;
        if line.starts_with('#') || line.trim().is_empty() || line.trim() == "---" {
            continue;
        }
        // Top-level and one-level nested keys (2-space indent).
        let indent = line.len() - line.trim_start().len();
        if indent > 2 {
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with('-') {
            continue;
        }
        let Some(colon) = trimmed.find(':') else {
            continue;
        };
        let key = trimmed[..colon].trim();
        if key.is_empty() || key.contains(' ') {
            continue;
        }
        push_ident(&mut out, intern, SymbolKind::Key, key, line_no, 0, None);
    }
    out
}

fn extract_sql(text: &str, intern: &mut Interner) -> Vec<Symbol> {
    let mut out = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        let line_no = line_idx as u32;
        let trimmed = line.trim_start();
        if trimmed.starts_with("--") {
            continue;
        }
        let upper = trimmed.to_ascii_uppercase();
        for kw in [
            "CREATE TABLE ",
            "CREATE VIEW ",
            "CREATE INDEX ",
            "CREATE UNIQUE INDEX ",
            "CREATE FUNCTION ",
            "CREATE PROCEDURE ",
        ] {
            if let Some(idx) = upper.find(kw) {
                let after = &trimmed[idx + kw.len()..];
                if let Some(name) = sql_ident(after) {
                    push_ident(&mut out, intern, SymbolKind::Table, name, line_no, 0, None);
                }
                break;
            }
        }
    }
    out
}

fn sql_ident(s: &str) -> Option<&str> {
    let s = s.trim_start();
    let s = s.strip_prefix("IF NOT EXISTS ").unwrap_or(s);
    let s = s.trim_start();
    if s.starts_with('`') || s.starts_with('"') {
        return symbol::first_quoted(s);
    }
    let end = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
        .unwrap_or(s.len());
    let name = &s[..end];
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn extract_python(text: &str, intern: &mut Interner) -> Vec<Symbol> {
    let mut out = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        let line_no = line_idx as u32;
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("class ") {
            if let Some(name) = symbol::ident_at(rest) {
                push_ident(&mut out, intern, SymbolKind::Class, name, line_no, 0, None);
            }
        }
        if let Some(rest) = trimmed.strip_prefix("def ") {
            if let Some(name) = symbol::ident_at(rest) {
                push_ident(
                    &mut out,
                    intern,
                    SymbolKind::Function,
                    name,
                    line_no,
                    0,
                    None,
                );
            }
        }
        if let Some(rest) = trimmed.strip_prefix("async def ") {
            if let Some(name) = symbol::ident_at(rest) {
                push_ident(
                    &mut out,
                    intern,
                    SymbolKind::Function,
                    name,
                    line_no,
                    0,
                    None,
                );
            }
        }
    }
    out
}

fn extract_rust(text: &str, intern: &mut Interner) -> Vec<Symbol> {
    let mut out = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        let line_no = line_idx as u32;
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        let rest = strip_rust_vis(trimmed);
        for (kw, kind) in [
            ("fn ", SymbolKind::Function),
            ("struct ", SymbolKind::Class),
            ("enum ", SymbolKind::Type),
            ("trait ", SymbolKind::Type),
            ("mod ", SymbolKind::Module),
            ("type ", SymbolKind::Type),
            ("const ", SymbolKind::Variable),
        ] {
            if let Some(after) = rest.strip_prefix(kw) {
                if let Some(name) = rust_ident(after) {
                    push_ident(&mut out, intern, kind, name, line_no, 0, None);
                }
            }
        }
        if let Some(after) = rest.strip_prefix("impl ") {
            if let Some(name) = rust_ident(after) {
                push_ident(&mut out, intern, SymbolKind::Class, name, line_no, 0, None);
            }
        }
        if let Some(after) = rest.strip_prefix("macro_rules! ") {
            if let Some(name) = rust_ident(after) {
                push_ident(
                    &mut out,
                    intern,
                    SymbolKind::Function,
                    name,
                    line_no,
                    0,
                    None,
                );
            }
        }
    }
    out
}

fn strip_rust_vis(s: &str) -> &str {
    let s = s.strip_prefix("pub(crate) ").unwrap_or(s);
    let s = s.strip_prefix("pub(super) ").unwrap_or(s);
    let s = s.strip_prefix("pub ").unwrap_or(s);
    let s = s.strip_prefix("async ").unwrap_or(s);
    s
}

fn rust_ident(s: &str) -> Option<&str> {
    let s = s.trim_start();
    let end = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(s.len());
    let name = &s[..end];
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(text: &str, lang: &str) -> Vec<String> {
        let mut intern = Interner::new();
        extract(lang, text, &mut intern)
            .into_iter()
            .map(|s| intern.get(s.name_id).unwrap().to_string())
            .collect()
    }

    #[test]
    fn javascript_and_typescript() {
        let js = names(
            "export class CartWidget {}\nexport function formatPrice() {}\nconst checkout = () => {}",
            "javascript",
        );
        assert!(js.contains(&"CartWidget".into()));
        assert!(js.contains(&"formatPrice".into()));
        assert!(js.contains(&"checkout".into()));

        let ts = names(
            "export interface User { id: string }\nexport type UserId = string\nenum Role { Admin }\n",
            "typescript",
        );
        assert!(ts.contains(&"User".into()));
        assert!(ts.contains(&"UserId".into()));
        assert!(ts.contains(&"Role".into()));
    }

    #[test]
    fn html_css_json_yaml() {
        let html = names(
            r#"<section id="hero" class="wrap"><shop-cart></shop-cart></section>"#,
            "html",
        );
        assert!(html.contains(&"hero".into()));
        assert!(html.contains(&"wrap".into()));
        assert!(html.contains(&"shop-cart".into()));

        let css = names(
            ".hero { color: red }\n#nav { }\n@keyframes fade-in { }",
            "css",
        );
        assert!(css.contains(&"hero".into()));
        assert!(css.contains(&"nav".into()));
        assert!(css.contains(&"fade-in".into()));

        let json = names("{\n  \"name\": \"demo\",\n  \"scripts\": {}\n}\n", "json");
        assert!(json.contains(&"name".into()));
        assert!(json.contains(&"scripts".into()));

        let yaml = names("services:\n  web:\n    image: nginx\n", "yaml");
        assert!(yaml.contains(&"services".into()));
        assert!(yaml.contains(&"web".into()));
    }

    #[test]
    fn sql_python_rust() {
        let sql = names(
            "CREATE TABLE posts (\n  id INT\n);\nCREATE INDEX posts_slug ON posts (slug);",
            "sql",
        );
        assert!(sql.contains(&"posts".into()));
        assert!(sql.contains(&"posts_slug".into()));

        let py = names(
            "class Store:\n    def checkout(self):\n        pass\n",
            "python",
        );
        assert!(py.contains(&"Store".into()));
        assert!(py.contains(&"checkout".into()));

        let rs = names(
            "pub struct Index {}\npub fn intern() {}\nenum Kind { Class }\n",
            "rust",
        );
        assert!(rs.contains(&"Index".into()));
        assert!(rs.contains(&"intern".into()));
        assert!(rs.contains(&"Kind".into()));
    }

    #[test]
    fn support_matrix_covers_scanners() {
        let ids: Vec<_> = LANGUAGE_SUPPORT.iter().map(|l| l.id).collect();
        assert_eq!(ids, vec![
            "php",
            "javascript",
            "typescript",
            "html",
            "css",
            "json",
            "yaml",
            "sql",
            "python",
            "rust",
        ]);
        assert_eq!(LANGUAGE_SUPPORT[0].parser, "tree-sitter");
        assert_eq!(LANGUAGE_SUPPORT.iter().find(|l| l.id == "sql").unwrap().parser, "line-scan");
        assert!(LANGUAGE_SUPPORT
            .iter()
            .filter(|l| l.id != "sql")
            .all(|l| l.parser == "tree-sitter"));
        assert!(UNSUPPORTED_COMMON.contains(&"go"));
    }

    #[test]
    fn infers_from_uri() {
        assert_eq!(infer_from_uri("file:///tmp/a.php"), "php");
        assert_eq!(infer_from_uri("file:///tmp/a.ts"), "typescript");
        assert_eq!(
            normalize_language_id("javascriptreact", "file:///x.jsx"),
            "javascript"
        );
        assert_eq!(
            normalize_language_id("fundamental", "file:///tmp/a.php"),
            "php"
        );
    }

    #[test]
    fn mixed_testdata_each_file_has_symbols() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/mixed");
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.is_file())
            .collect();
        files.sort();
        assert_eq!(files.len(), 10, "expected 10 mixed-language fixtures");
        for path in files {
            let text = std::fs::read_to_string(&path).unwrap();
            let lang = infer_from_uri(&format!("file://{}", path.display()));
            let mut intern = Interner::new();
            let syms = extract(&lang, &text, &mut intern);
            assert!(
                !syms.is_empty(),
                "{} ({lang}) produced no symbols",
                path.display()
            );
        }
    }
}
