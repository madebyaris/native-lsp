//! Host profile from research §8. Missing GPU steps are skipped, not errors.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HostProfile {
    Cpu,
    AppleUnified,
    NvidiaDiscrete,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IoMode {
    Mmap,
    StreamPread,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostInfo {
    pub profile: HostProfile,
    pub io: IoMode,
    pub os: String,
    pub arch: String,
}

pub fn detect() -> HostInfo {
    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();

    let profile = if os == "macos" {
        HostProfile::AppleUnified
    } else if nvidia_present() {
        HostProfile::NvidiaDiscrete
    } else if os == "linux" {
        HostProfile::Cpu
    } else {
        HostProfile::Unknown
    };

    let io = match std::env::var("NATIVE_LSP_IO").ok().as_deref() {
        Some("pread") | Some("stream") => IoMode::StreamPread,
        Some("mmap") => IoMode::Mmap,
        _ if looks_like_network_fs() => IoMode::StreamPread,
        _ => IoMode::Mmap,
    };

    HostInfo {
        profile,
        io,
        os,
        arch,
    }
}

fn nvidia_present() -> bool {
    std::path::Path::new("/dev/nvidia0").exists()
        || std::path::Path::new("/proc/driver/nvidia/version").exists()
}

fn looks_like_network_fs() -> bool {
    // SSH/NFS remote workspaces often set these; mmap is hostile there.
    std::env::var_os("SSH_CONNECTION").is_some()
        || std::env::var("NATIVE_LSP_IO").as_deref() == Ok("stream")
}
