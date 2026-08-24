//! Process RSS. Linux `/proc`; other OS returns `None`.

pub fn rss_bytes() -> Option<u64> {
    rss_bytes_of("self")
}

pub fn rss_bytes_of(pid: impl AsRef<str>) -> Option<u64> {
    let path = format!("/proc/{}/status", pid.as_ref());
    let text = std::fs::read_to_string(path).ok()?;
    for line in text.lines() {
        let rest = line.strip_prefix("VmRSS:")?;
        let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
        return Some(kb.saturating_mul(1024));
    }
    None
}

pub fn format_mb(bytes: u64) -> String {
    format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
}
