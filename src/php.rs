//! Lightweight PHP / WordPress symbol scan. Tree-sitter can replace this later.

use crate::intern::Interner;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhpKind {
    Class,
    Function,
    Hook,
}

#[derive(Debug, Clone)]
pub struct PhpSymbol {
    pub name_id: u32,
    pub kind: PhpKind,
    pub line: u32,
    pub character: u32,
    /// Interned hook name when `kind == Hook`.
    pub hook_id: Option<u32>,
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

pub fn extract(text: &str, intern: &mut Interner) -> Vec<PhpSymbol> {
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

fn scan_class(text: &str, intern: &mut Interner, line_no: u32) -> Option<PhpSymbol> {
    let trimmed = text.trim_start();
    let rest = trimmed.strip_prefix("class ")?;
    let name = ident_at(rest)?;
    let character = (text.len() - trimmed.len() + 6) as u32;
    Some(PhpSymbol {
        name_id: intern.intern(name),
        kind: PhpKind::Class,
        line: line_no,
        character,
        hook_id: None,
    })
}

fn scan_function(text: &str, intern: &mut Interner, line_no: u32) -> Option<PhpSymbol> {
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
    Some(PhpSymbol {
        name_id: intern.intern(name),
        kind: PhpKind::Function,
        line: line_no,
        character: 0,
        hook_id: None,
    })
}

fn scan_hooks(text: &str, intern: &mut Interner, line_no: u32) -> Vec<PhpSymbol> {
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
                out.push(PhpSymbol {
                    name_id: intern.intern(name),
                    kind: PhpKind::Hook,
                    line: line_no,
                    character: 0,
                    hook_id: Some(intern.intern(hook)),
                });
            }
            search = &search[idx + call.len()..];
        }
    }
    out
}

fn ident_at(s: &str) -> Option<&str> {
    let s = s.trim_start();
    let end = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '\\'))
        .unwrap_or(s.len());
    let name = &s[..end];
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn first_quoted(s: &str) -> Option<&str> {
    let s = s.trim_start();
    let quote = s.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let rest = &s[1..];
    let end = rest.find(quote)?;
    Some(&rest[..end])
}

pub fn symbol_at_line<'a>(symbols: &'a [PhpSymbol], line: u32) -> Option<&'a PhpSymbol> {
    symbols.iter().rev().find(|s| s.line == line)
}

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
            .filter_map(|s| s.hook_id.and_then(|id| intern.get(id)))
            .collect();
        assert_eq!(hooks, vec!["init"]);
    }
}
