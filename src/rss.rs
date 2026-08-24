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
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return None;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        if looks_like_lsp(pid) {
            return Some(pid);
        }
    }
    None
}

pub fn is_lsp_pid(pid: u32) -> bool {
    cmdline_looks_like_lsp(&comm(pid), &cmdline(pid))
}

/// Token match: the binary path, not `cargo --bin native-lsp` or a temp dir.
pub fn cmdline_looks_like_lsp(comm: &str, cmdline: &str) -> bool {
    if comm == "native-lsp" {
        return true;
    }
    cmdline
        .split_whitespace()
        .any(|part| part.ends_with("/native-lsp") || part.ends_with("node-lsp.mjs"))
}

fn looks_like_lsp(pid: u32) -> bool {
    is_lsp_pid(pid)
}

/// One `/proc` row for editor-tree dumps (VS Code Electron especially).
#[derive(Debug, Clone)]
pub struct ProcSample {
    pub pid: u32,
    pub comm: String,
    pub role: String,
    pub rss_bytes: u64,
}

pub fn sample_procs(pids: &[u32]) -> Vec<ProcSample> {
    let mut rows: Vec<ProcSample> = pids
        .iter()
        .copied()
        .filter_map(|pid| {
            Some(ProcSample {
                pid,
                comm: comm(pid),
                role: process_role(pid),
                rss_bytes: rss_bytes_of(pid.to_string())?,
            })
        })
        .collect();
    rows.sort_by(|a, b| b.rss_bytes.cmp(&a.rss_bytes).then(a.pid.cmp(&b.pid)));
    rows
}

pub fn process_role(pid: u32) -> String {
    if is_lsp_pid(pid) {
        if comm(pid) == "native-lsp" {
            return "language-server (native-lsp)".into();
        }
        return "language-server (node-lsp)".into();
    }
    let comm_name = comm(pid);
    if comm_name.contains("crashpad") {
        return "crashpad".into();
    }
    let cmd = cmdline(pid);
    if let Some(kind) = electron_type(&cmd) {
        return match kind {
            "gpu-process" => "gpu-process (Chromium)".into(),
            "renderer" => "renderer (Monaco / workbench)".into(),
            "utility" => utility_role(&cmd),
            "crashpad-handler" => "crashpad".into(),
            "zygote" => "zygote".into(),
            "broker" => "broker".into(),
            other => other.to_string(),
        };
    }
    "main (Electron)".into()
}

fn electron_type(cmd: &str) -> Option<&str> {
    // Prefer the Chromium --type= flag. Do not match --crashpad-handler-pid=.
    for part in cmd.split_whitespace() {
        if let Some(kind) = part.strip_prefix("--type=") {
            return Some(kind);
        }
    }
    None
}

fn utility_role(cmd: &str) -> String {
    let sub = cmd
        .split_whitespace()
        .find_map(|p| p.strip_prefix("--utility-sub-type="))
        .unwrap_or("");
    if sub.contains("NodeService") {
        if cmd.contains("--inspect-port") {
            return "extensionHost (Node)".into();
        }
        return "node utility (Electron)".into();
    }
    if sub.contains("NetworkService") {
        return "network utility".into();
    }
    if sub.is_empty() {
        "utility".into()
    } else {
        format!("utility ({sub})")
    }
}

pub fn format_proc_table(rows: &[ProcSample]) -> String {
    let mut out = String::from("| pid | role | RSS | comm |\n| ---: | --- | ---: | --- |\n");
    for row in rows {
        out.push_str(&format!(
            "| {} | {} | {} | `{}` |\n",
            row.pid,
            row.role,
            format_mb(row.rss_bytes),
            row.comm.replace('|', " ")
        ));
    }
    out
}

pub fn format_role_totals(rows: &[ProcSample]) -> String {
    use std::collections::BTreeMap;
    let mut totals: BTreeMap<&str, (u64, usize)> = BTreeMap::new();
    for row in rows {
        let entry = totals.entry(row.role.as_str()).or_insert((0, 0));
        entry.0 += row.rss_bytes;
        entry.1 += 1;
    }
    let mut out = String::from("| role | processes | RSS |\n| --- | ---: | ---: |\n");
    let mut ranked: Vec<_> = totals.into_iter().collect();
    ranked.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    for (role, (bytes, n)) in ranked {
        out.push_str(&format!("| {role} | {n} | {} |\n", format_mb(bytes)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_rss_is_nonzero() {
        let rss = rss_bytes().expect("VmRSS from /proc/self/status");
        assert!(rss > 1024, "rss was {rss}");
    }

    #[test]
    fn lsp_match_is_the_binary_not_cargo_args() {
        assert!(cmdline_looks_like_lsp("native-lsp", "/workspace/target/release/native-lsp"));
        assert!(cmdline_looks_like_lsp(
            "node",
            "node /workspace/compare/node-lsp.mjs"
        ));
        assert!(!cmdline_looks_like_lsp(
            "bash",
            "cargo build --release --bin native-lsp --bin compare-hosts"
        ));
        assert!(!cmdline_looks_like_lsp(
            "code",
            "code --user-data-dir=/tmp/native-lsp-vscode-1/user"
        ));
    }
}
