//! Shared fixture loading for compare-rss and native-ide.

use std::path::{Path, PathBuf};

use crate::lang;

#[derive(Debug, Clone)]
pub struct OpenFile {
    pub path: PathBuf,
    pub language_id: String,
}

pub fn profile_dir() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

pub fn default_native_lsp() -> PathBuf {
    std::env::var("NATIVE_LSP_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join(profile_dir())
                .join("native-lsp")
        })
}

pub fn default_native_ide() -> PathBuf {
    std::env::var("NATIVE_IDE_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join(profile_dir())
                .join("native-ide")
        })
}

pub fn default_node_bin() -> String {
    std::env::var("NODE_BIN").unwrap_or_else(|_| "node".into())
}

pub fn default_node_script() -> PathBuf {
    std::env::var("NODE_LSP_SCRIPT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("compare/node-lsp.mjs"))
}

pub fn mixed_dir() -> PathBuf {
    std::env::var("NATIVE_LSP_MIXED")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/mixed"))
}

pub fn wp_plugin_dir() -> PathBuf {
    std::env::var("NATIVE_LSP_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/wp-plugin"))
}

pub fn workspace_files(dir: &Path) -> Result<Vec<OpenFile>, Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    walk_files(dir, dir, &mut files)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    if files.is_empty() {
        return Err(format!("no language files in {}", dir.display()).into());
    }
    Ok(files)
}

fn walk_files(root: &Path, dir: &Path, files: &mut Vec<OpenFile>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            if path.file_name().and_then(|s| s.to_str()) == Some("node_modules") {
                continue;
            }
            walk_files(root, &path, files)?;
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let language_id = lang::infer_from_uri(&format!(
            "file:///{}",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
        if language_id == "plaintext" {
            continue;
        }
        files.push(OpenFile { path, language_id });
    }
    Ok(())
}

pub fn mixed_files() -> Result<Vec<OpenFile>, Box<dyn std::error::Error>> {
    mixed_files_in(&mixed_dir())
}

pub fn mixed_files_in(dir: &Path) -> Result<Vec<OpenFile>, Box<dyn std::error::Error>> {
    if !dir.is_dir() {
        return Err(format!("missing mixed fixtures at {}", dir.display()).into());
    }
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let language_id = lang::infer_from_uri(&format!(
            "file:///{}",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
        files.push(OpenFile { path, language_id });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    if files.len() != 10 {
        return Err(format!(
            "expected 10 mixed files, found {} in {}",
            files.len(),
            dir.display()
        )
        .into());
    }
    Ok(files)
}

pub fn php_files(dir: &Path) -> std::io::Result<Vec<OpenFile>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) == Some("php") {
            files.push(OpenFile {
                path,
                language_id: "php".into(),
            });
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

pub fn write_php_fixtures(dir: &Path, count: usize) -> std::io::Result<()> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    std::fs::create_dir_all(dir)?;
    for i in 0..count {
        let body = format!(
            r#"<?php
/**
 * Generated fixture file {i} for RSS comparison.
 */
class Fixture_Plugin_{i} {{
    public function boot() {{
        add_action('init', [$this, 'boot']);
        add_filter('the_content', [$this, 'filter_content']);
    }}

    public function filter_content($content) {{
        return $content . ' {i}';
    }}
}}

function fixture_{i}_helper() {{
    $q = new WP_Query(['post_type' => 'post']);
    return get_option('fixture_{i}');
}}
"#
        );
        std::fs::write(dir.join(format!("fixture-{i:03}.php")), body)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_dir_has_ten_languages() {
        let files = mixed_files().expect("testdata/mixed");
        assert_eq!(files.len(), 10);
        let langs: Vec<_> = files.iter().map(|f| f.language_id.as_str()).collect();
        assert!(langs.contains(&"php"));
        assert!(langs.contains(&"rust"));
        assert!(langs.contains(&"typescript"));
    }

    #[test]
    fn wp_plugin_is_a_real_workspace() {
        let files = workspace_files(&wp_plugin_dir()).expect("testdata/wp-plugin");
        assert!(files.len() >= 10, "got {} files", files.len());
        let langs: Vec<_> = files.iter().map(|f| f.language_id.as_str()).collect();
        assert!(langs.contains(&"php"));
        assert!(langs.contains(&"javascript"));
        assert!(langs.contains(&"typescript"));
        assert!(langs.contains(&"html"));
        assert!(langs.contains(&"css"));
    }
}
