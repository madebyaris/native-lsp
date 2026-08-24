//! Line-scan fallback for PHP / WordPress. Open files prefer `cst` (tree-sitter).

use crate::intern::Interner;
use crate::symbol::{first_quoted, ident_at, Symbol, SymbolKind};

pub fn extract(text: &str, intern: &mut Interner) -> Vec<Symbol> {
    let mut out = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        let line_no = line_idx as u32;
        if let Some(sym) = scan_class(line, intern, line_no) {
            out.push(sym);
        }
        if let Some(sym) = scan_function(line, intern, line_no) {
            out.push(sym);
        }
        out.extend(scan_hooks(line, intern, line_no));
    }
    out
}

fn scan_class(text: &str, intern: &mut Interner, line_no: u32) -> Option<Symbol> {
    let trimmed = text.trim_start();
    let rest = trimmed.strip_prefix("class ")?;
    let name = ident_at(rest)?;
    let character = (text.len() - trimmed.len() + 6) as u32;
    Some(Symbol {
        name_id: intern.intern(name),
        kind: SymbolKind::Class,
        line: line_no,
        character,
        extra_id: None,
    })
}

fn scan_function(text: &str, intern: &mut Interner, line_no: u32) -> Option<Symbol> {
    let trimmed = text.trim_start();
    // Skip anonymous / closures roughly.
    let rest = if let Some(r) = trimmed.strip_prefix("public function ") {
        r
    } else if let Some(r) = trimmed.strip_prefix("private function ") {
        r
    } else if let Some(r) = trimmed.strip_prefix("protected function ") {
        r
    } else if let Some(r) = trimmed.strip_prefix("static function ") {
        r
    } else if let Some(r) = trimmed.strip_prefix("function ") {
        r
    } else {
        return None;
    };
    if rest.starts_with('(') {
        return None;
    }
    let name = ident_at(rest)?;
    if name == "if" || name == "foreach" {
        return None;
    }
    Some(Symbol {
        name_id: intern.intern(name),
        kind: SymbolKind::Function,
        line: line_no,
        character: 0,
        extra_id: None,
    })
}

fn scan_hooks(text: &str, intern: &mut Interner, line_no: u32) -> Vec<Symbol> {
    let mut out = Vec::new();
    for call in ["add_action(", "add_filter("] {
        let mut search = text;
        while let Some(idx) = search.find(call) {
            let after = &search[idx + call.len()..];
            if let Some(hook) = first_quoted(after) {
                let name = if call.starts_with("add_action") {
                    "add_action"
                } else {
                    "add_filter"
                };
                out.push(Symbol {
                    name_id: intern.intern(name),
                    kind: SymbolKind::Hook,
                    line: line_no,
                    character: 0,
                    extra_id: Some(intern.intern(hook)),
                });
            }
            search = &search[idx + call.len()..];
        }
    }
    out
}

pub const WP_STUBS: &[&str] = &[
    "WP_Query",
    "WP_Post",
    "WP_User",
    "wpdb",
    "get_option",
    "update_option",
    "get_post",
    "get_posts",
    "wp_enqueue_script",
    "wp_enqueue_style",
    "add_action",
    "add_filter",
    "apply_filters",
    "do_action",
    "register_post_type",
    "register_taxonomy",
    "plugin_dir_path",
    "plugin_dir_url",
    "__",
    "esc_html",
    "esc_attr",
    "wp_die",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_class_function_and_hook() {
        let src = r#"
class Demo_Plugin {
    public function boot() {
        add_action('init', [$this, 'boot']);
    }
}
function demo_helper() {}
"#;
        let mut intern = Interner::new();
        let syms = extract(src, &mut intern);
        let names: Vec<_> = syms
            .iter()
            .map(|s| intern.get(s.name_id).unwrap())
            .collect();
        assert!(names.contains(&"Demo_Plugin"));
        assert!(names.contains(&"boot"));
        assert!(names.contains(&"demo_helper"));
        assert!(names.contains(&"add_action"));
        let hooks: Vec<_> = syms
            .iter()
            .filter_map(|s| s.extra_id.and_then(|id| intern.get(id)))
            .collect();
        assert_eq!(hooks, vec!["init"]);
    }
}
