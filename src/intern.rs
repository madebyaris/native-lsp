//! String intern table: one copy of each name, `u32` IDs everywhere else.

use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Default)]
pub struct Interner {
    map: HashMap<String, u32>,
    strings: Vec<String>,
}

impl Interner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.map.get(s) {
            return id;
        }
        let id = self.strings.len() as u32;
        self.strings.push(s.to_string());
        self.map.insert(s.to_string(), id);
        id
    }

    pub fn get(&self, id: u32) -> Option<&str> {
        self.strings.get(id as usize).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.strings.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }

    /// Compact snapshot: `NLS1` + u32 count + (u32 len + bytes)*n. Sorted on write
    /// so a later compressor would see long runs of shared prefixes.
    pub fn write_snapshot(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut ordered = self.strings.clone();
        ordered.sort();
        let mut buf = Vec::new();
        buf.extend_from_slice(b"NLS1");
        buf.extend_from_slice(&(ordered.len() as u32).to_le_bytes());
        for s in &ordered {
            let bytes = s.as_bytes();
            buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(bytes);
        }
        std::fs::write(path, buf)
    }

    pub fn load_snapshot(path: &Path) -> std::io::Result<Self> {
        let data = std::fs::read(path)?;
        if data.len() < 8 || &data[..4] != b"NLS1" {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "bad native-lsp snapshot magic",
            ));
        }
        let count = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
        let mut intern = Self::new();
        let mut i = 8;
        for _ in 0..count {
            if i + 4 > data.len() {
                break;
            }
            let len = u32::from_le_bytes(data[i..i + 4].try_into().unwrap()) as usize;
            i += 4;
            if i + len > data.len() {
                break;
            }
            let s = std::str::from_utf8(&data[i..i + len])
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            intern.intern(s);
            i += len;
        }
        Ok(intern)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_roundtrip() {
        let mut intern = Interner::new();
        intern.intern("WP_Query");
        intern.intern("add_action");
        intern.intern("WP_Query");
        assert_eq!(intern.len(), 2);

        let dir = std::env::temp_dir().join("native-lsp-intern-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("index.nls1");
        intern.write_snapshot(&path).unwrap();
        let loaded = Interner::load_snapshot(&path).unwrap();
        assert_eq!(loaded.len(), 2);
        assert!(loaded.get(0) == Some("WP_Query") || loaded.get(0) == Some("add_action"));
        assert!(loaded.get(1) == Some("WP_Query") || loaded.get(1) == Some("add_action"));
    }
}
