//! Interned symbols shared by every language scanner.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Class,
    Function,
    Hook,
    Type,
    Variable,
    Tag,
    Rule,
    Key,
    Table,
    Module,
}

impl SymbolKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Function => "function",
            Self::Hook => "hook",
            Self::Type => "type",
            Self::Variable => "variable",
            Self::Tag => "tag",
            Self::Rule => "rule",
            Self::Key => "key",
            Self::Table => "table",
            Self::Module => "module",
        }
    }

    /// LSP `SymbolKind` numeric values.
    pub fn lsp_kind(self) -> u8 {
        match self {
            Self::Class => 5,
            Self::Function => 12,
            Self::Hook => 24,
            Self::Type => 11,
            Self::Variable => 13,
            Self::Tag => 5,
            Self::Rule => 7,
            Self::Key => 8,
            Self::Table => 23,
            Self::Module => 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub name_id: u32,
    pub kind: SymbolKind,
    pub line: u32,
    pub character: u32,
    /// Hook name, CSS property, or other language-specific extra.
    pub extra_id: Option<u32>,
}

pub fn symbol_at_line(symbols: &[Symbol], line: u32) -> Option<&Symbol> {
    symbols.iter().find(|s| s.line == line)
}

pub fn ident_at(s: &str) -> Option<&str> {
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

pub fn first_quoted(s: &str) -> Option<&str> {
    let s = s.trim_start();
    let quote = s.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let rest = &s[1..];
    let end = rest.find(quote)?;
    Some(&rest[..end])
}

pub fn push_ident(
    out: &mut Vec<Symbol>,
    intern: &mut crate::intern::Interner,
    kind: SymbolKind,
    name: &str,
    line: u32,
    character: u32,
    extra: Option<&str>,
) {
    out.push(Symbol {
        name_id: intern.intern(name),
        kind,
        line,
        character,
        extra_id: extra.map(|e| intern.intern(e)),
    });
}
