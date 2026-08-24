//! Tiny native workbench: same IDE process, swap only the LSP child.
//!
//! This is the fair A/B. The editor RSS is held constant; we report IDE, LSP,
//! and total separately. It is not a comparison against VS Code or Intelephense.

use std::io::{self, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::client::{self, LspClient};
use crate::fixture::{self, OpenFile};
use crate::rss;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LspKind {
    Native,
    Node,
}

impl LspKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "native" | "native-lsp" => Some(Self::Native),
            "node" | "node-lsp" => Some(Self::Node),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Node => "node",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabProbe {
    pub name: String,
    pub language_id: String,
    pub symbol_count: usize,
    pub hover: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdeReport {
    pub server: String,
    pub ide_pid: u32,
    pub lsp_pid: u32,
    pub idle_ide_bytes: u64,
    pub idle_lsp_bytes: u64,
    pub after_open_ide_bytes: u64,
    pub after_open_lsp_bytes: u64,
    pub after_tabs_ide_bytes: u64,
    pub after_tabs_lsp_bytes: u64,
    pub hover_ms: u64,
    pub tabs: Vec<TabProbe>,
}

impl IdeReport {
    pub fn idle_total(&self) -> u64 {
        self.idle_ide_bytes.saturating_add(self.idle_lsp_bytes)
    }

    pub fn after_open_total(&self) -> u64 {
        self.after_open_ide_bytes
            .saturating_add(self.after_open_lsp_bytes)
    }

    pub fn after_tabs_total(&self) -> u64 {
        self.after_tabs_ide_bytes
            .saturating_add(self.after_tabs_lsp_bytes)
    }
}

struct Buffer {
    file: OpenFile,
    text: String,
    uri: String,
}

pub struct Workbench {
    kind: LspKind,
    buffers: Vec<Buffer>,
    active: usize,
    child: Child,
    client: LspClient,
    lsp_pid: u32,
}

impl Workbench {
    pub fn start(kind: LspKind, files: Vec<OpenFile>) -> client::Result<Self> {
        let mut buffers = Vec::new();
        for file in files {
            let text = std::fs::read_to_string(&file.path)?;
            let uri = client::path_uri(&file.path);
            buffers.push(Buffer { file, text, uri });
        }
        if buffers.is_empty() {
            return Err("no files to open".into());
        }

        let mut child = spawn_lsp(kind)?;
        let lsp_pid = child.id();
        let stdin = child.stdin.take().expect("lsp stdin");
        let stdout = child.stdout.take().expect("lsp stdout");
        let mut client = LspClient::new(stdin, stdout);

        client.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "capabilities": {},
                "rootUri": null,
            }),
        )?;
        client.notify("initialized", json!({}))?;

        Ok(Self {
            kind,
            buffers,
            active: 0,
            child,
            client,
            lsp_pid,
        })
    }

    pub fn lsp_pid(&self) -> u32 {
        self.lsp_pid
    }

    pub fn active(&self) -> usize {
        self.active
    }

    pub fn buffer_count(&self) -> usize {
        self.buffers.len()
    }

    pub fn did_open_all(&mut self) -> client::Result<()> {
        for buf in &self.buffers {
            self.client.notify(
                "textDocument/didOpen",
                json!({
                    "textDocument": {
                        "uri": buf.uri,
                        "languageId": buf.file.language_id,
                        "version": 1,
                        "text": buf.text
                    }
                }),
            )?;
        }
        Ok(())
    }

    /// Tab switch: active buffer stays open; hidden buffers nap. This is the
    /// IDE clock from research §8 — not `didClose`.
    pub fn activate(&mut self, index: usize) -> client::Result<()> {
        if index >= self.buffers.len() {
            return Err(format!("tab {index} out of range").into());
        }
        self.active = index;
        for (i, buf) in self.buffers.iter().enumerate() {
            let state = if i == index { "active" } else { "hidden" };
            self.client.notify(
                "$/nativeLsp/documentVisibility",
                json!({ "uri": buf.uri, "state": state }),
            )?;
        }
        self.client.notify("$/nativeLsp/wake", json!({}))?;
        Ok(())
    }

    pub fn probe_active(&mut self) -> client::Result<(TabProbe, u64)> {
        let buf = &self.buffers[self.active];
        let symbols = self.client.request(
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": buf.uri } }),
        )?;
        let symbol_count = symbols.as_array().map(|a| a.len()).unwrap_or(0);
        let line = symbols
            .as_array()
            .and_then(|a| a.first())
            .and_then(|s| s.pointer("/location/range/start/line"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let t0 = Instant::now();
        let hover = self.client.request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": buf.uri },
                "position": { "line": line, "character": 0 }
            }),
        )?;
        let hover_ms = t0.elapsed().as_millis() as u64;
        let hover_text = hover
            .pointer("/contents/value")
            .and_then(Value::as_str)
            .unwrap_or("")
            .lines()
            .next()
            .unwrap_or("")
            .to_string();
        Ok((
            TabProbe {
                name: buf
                    .file
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                language_id: buf.file.language_id.clone(),
                symbol_count,
                hover: hover_text,
            },
            hover_ms,
        ))
    }

    pub fn render_frame(&self, probe: Option<&TabProbe>, ide_rss: u64, lsp_rss: u64) -> String {
        let buf = &self.buffers[self.active];
        let mut tabs = String::new();
        for (i, b) in self.buffers.iter().enumerate() {
            let name = b
                .file
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            if i == self.active {
                tabs.push_str(&format!("[{}]", name));
            } else {
                tabs.push_str(&format!(" {} ", name));
            }
            if i + 1 != self.buffers.len() {
                tabs.push(' ');
            }
        }
        let mut out = String::new();
        out.push_str(&format!(
            "native-ide  lsp={}  ide={}  lsp_rss={}  total={}\n",
            self.kind.as_str(),
            rss::format_mb(ide_rss),
            rss::format_mb(lsp_rss),
            rss::format_mb(ide_rss.saturating_add(lsp_rss))
        ));
        out.push_str("tabs: ");
        out.push_str(&tabs);
        out.push('\n');
        out.push_str(&format!(
            "active: {} ({})\n",
            buf.file.path.display(),
            buf.file.language_id
        ));
        out.push_str("----\n");
        for (i, line) in buf.text.lines().take(14).enumerate() {
            out.push_str(&format!("{:>4}  {}\n", i + 1, line));
        }
        out.push_str("----\n");
        if let Some(p) = probe {
            out.push_str(&format!(
                "hover: {}\nsymbols: {} names\n",
                if p.hover.is_empty() {
                    "(none)"
                } else {
                    p.hover.as_str()
                },
                p.symbol_count
            ));
        }
        out
    }

    pub fn shutdown(mut self) -> client::Result<()> {
        let _ = self.client.request("shutdown", json!(null));
        let _ = self.client.notify("exit", json!(null));
        drop(self.client);
        let _ = self.child.wait();
        Ok(())
    }
}

pub fn run_once(kind: LspKind, files: Vec<OpenFile>) -> client::Result<IdeReport> {
    let ide_pid = std::process::id();
    let mut wb = Workbench::start(kind, files)?;
    let lsp_pid = wb.lsp_pid();
    std::thread::sleep(Duration::from_millis(50));

    let idle_ide = sample_self()?;
    let idle_lsp = sample_pid(lsp_pid)?;

    wb.did_open_all()?;
    std::thread::sleep(Duration::from_millis(40));
    let after_open_ide = sample_self()?;
    let after_open_lsp = sample_pid(lsp_pid)?;

    let mut tabs = Vec::new();
    let mut hover_ms = 0u64;
    for i in 0..wb.buffer_count() {
        wb.activate(i)?;
        let (probe, ms) = wb.probe_active()?;
        hover_ms += ms;
        tabs.push(probe);
    }
    std::thread::sleep(Duration::from_millis(40));
    let after_tabs_ide = sample_self()?;
    let after_tabs_lsp = sample_pid(lsp_pid)?;

    let report = IdeReport {
        server: kind.as_str().into(),
        ide_pid,
        lsp_pid,
        idle_ide_bytes: idle_ide,
        idle_lsp_bytes: idle_lsp,
        after_open_ide_bytes: after_open_ide,
        after_open_lsp_bytes: after_open_lsp,
        after_tabs_ide_bytes: after_tabs_ide,
        after_tabs_lsp_bytes: after_tabs_lsp,
        hover_ms,
        tabs,
    };
    wb.shutdown()?;
    Ok(report)
}

pub fn run_repl(kind: LspKind, files: Vec<OpenFile>) -> client::Result<()> {
    let mut wb = Workbench::start(kind, files)?;
    wb.did_open_all()?;
    wb.activate(0)?;
    let (mut probe, _) = wb.probe_active()?;
    loop {
        let ide = rss::rss_bytes().unwrap_or(0);
        let lsp = rss::rss_bytes_of(wb.lsp_pid().to_string()).unwrap_or(0);
        print!("{}", wb.render_frame(Some(&probe), ide, lsp));
        println!("commands: tab N | hover | rss | quit");
        print!("> ");
        io::stdout().flush()?;
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == "quit" || line == "q" {
            break;
        }
        if line == "rss" {
            continue;
        }
        if line == "hover" {
            let (p, _) = wb.probe_active()?;
            probe = p;
            continue;
        }
        if let Some(rest) = line.strip_prefix("tab ") {
            let n: usize = rest.trim().parse().map_err(|_| "tab N")?;
            let idx = n.saturating_sub(1);
            wb.activate(idx)?;
            let (p, _) = wb.probe_active()?;
            probe = p;
            continue;
        }
        println!("unknown command");
    }
    wb.shutdown()?;
    Ok(())
}

pub fn print_report(report: &IdeReport) {
    println!();
    println!("## native-ide + {}-lsp", report.server);
    println!();
    println!("pids: ide {}, lsp {}", report.ide_pid, report.lsp_pid);
    println!();
    println!("| stage | IDE | LSP | total |");
    println!("| --- | ---: | ---: | ---: |");
    println!(
        "| idle (buffers loaded, LSP initialized) | {} | {} | {} |",
        rss::format_mb(report.idle_ide_bytes),
        rss::format_mb(report.idle_lsp_bytes),
        rss::format_mb(report.idle_total())
    );
    println!(
        "| after didOpen all tabs | {} | {} | {} |",
        rss::format_mb(report.after_open_ide_bytes),
        rss::format_mb(report.after_open_lsp_bytes),
        rss::format_mb(report.after_open_total())
    );
    println!(
        "| after cycling tabs (visibility + hover) | {} | {} | {} |",
        rss::format_mb(report.after_tabs_ide_bytes),
        rss::format_mb(report.after_tabs_lsp_bytes),
        rss::format_mb(report.after_tabs_total())
    );
    println!();
    println!("| tab | language | symbols | hover |");
    println!("| --- | --- | ---: | --- |");
    for (i, tab) in report.tabs.iter().enumerate() {
        let hover = if tab.hover.is_empty() {
            "_none_".into()
        } else {
            format!("`{}`", tab.hover.replace('|', "\\|").replace('`', "'"))
        };
        println!(
            "| {} `{}` | {} | {} | {} |",
            i + 1,
            tab.name,
            tab.language_id,
            tab.symbol_count,
            hover
        );
    }
    println!();
}

fn spawn_lsp(kind: LspKind) -> client::Result<Child> {
    match kind {
        LspKind::Native => {
            let bin = fixture::default_native_lsp();
            if !bin.exists() {
                return Err(format!(
                    "native-lsp not found at {}. Build with `cargo build --release --bin native-lsp`.",
                    bin.display()
                )
                .into());
            }
            Ok(Command::new(bin)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()?)
        }
        LspKind::Node => {
            let script = fixture::default_node_script();
            if !script.exists() {
                return Err(format!("missing {}", script.display()).into());
            }
            Ok(Command::new(fixture::default_node_bin())
                .arg(script)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()?)
        }
    }
}

fn sample_self() -> client::Result<u64> {
    rss::rss_bytes().ok_or_else(|| "could not read IDE RSS".into())
}

fn sample_pid(pid: u32) -> client::Result<u64> {
    rss::rss_bytes_of(pid.to_string())
        .ok_or_else(|| format!("could not read LSP RSS for pid {pid}").into())
}

pub fn files_from_dir(dir: Option<&Path>) -> client::Result<Vec<OpenFile>> {
    match dir {
        Some(dir) => fixture::mixed_files_in(dir),
        None => fixture::mixed_files(),
    }
}
