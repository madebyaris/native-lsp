//! Process RSS. Linux `/proc`; other OS returns `None`.

pub fn rss_bytes() -> Option<u64> {
    rss_bytes_of("self")
}

pub fn rss_bytes_of(pid: impl AsRef<str>) -> Option<u64> {
    let path = format!("/proc/{}/status", pid.as_ref());
    let text = std::fs::read_to_string(path).ok()?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
            return Some(kb.saturating_mul(1024));
        }
    }
    None
}

pub fn format_mb(bytes: u64) -> String {
    format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
}

pub fn child_pids(pid: u32) -> Vec<u32> {
    let path = format!("/proc/{pid}/task/{pid}/children");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect()
}

pub fn descendants(pid: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut stack = child_pids(pid);
    while let Some(child) = stack.pop() {
        out.push(child);
        stack.extend(child_pids(child));
    }
    out
}

pub fn cmdline(pid: u32) -> String {
    std::fs::read(format!("/proc/{pid}/cmdline"))
        .map(|bytes| String::from_utf8_lossy(&bytes).replace('\0', " "))
        .unwrap_or_default()
}

pub fn comm(pid: u32) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .unwrap_or_default()
        .trim()
        .to_string()
}

pub fn pids_with_cmdline(needle: &str) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut pids = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        if cmdline(pid).contains(needle) {
            pids.push(pid);
        }
    }
    pids
}

/// Prefer the language-server child, not the editor.
pub fn find_lsp_pid(host_pid: u32) -> Option<u32> {
    for pid in descendants(host_pid) {
        if looks_like_lsp(pid) {
            return Some(pid);
        }
    }
    None
}

pub fn find_lsp_pid_global() -> Option<u32> {
    pids_with_cmdline("native-lsp")
        .into_iter()
        .chain(pids_with_cmdline("node-lsp.mjs"))
        .find(|&pid| looks_like_lsp(pid))
}

fn looks_like_lsp(pid: u32) -> bool {
    let c = comm(pid);
    let cmd = cmdline(pid);
    c == "native-lsp"
        || cmd.contains("/native-lsp")
        || cmd.contains("native-lsp ")
        || cmd.contains("node-lsp.mjs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_rss_is_nonzero() {
        let rss = rss_bytes().expect("VmRSS from /proc/self/status");
        assert!(rss > 1024, "rss was {rss}");
    }
}
