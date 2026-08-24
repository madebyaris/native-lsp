//! Compare native-lsp RSS against a Node.js LSP with the same protocol surface.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

fn main() {
    if let Err(err) = run() {
        eprintln!("compare-rss failed: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let file_count: usize = std::env::var("COMPARE_FILES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(80);

    let native_bin = std::env::var("NATIVE_LSP_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_native_bin());
    let node_bin = std::env::var("NODE_BIN").unwrap_or_else(|_| "node".into());
    let node_script = std::env::var("NODE_LSP_SCRIPT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("compare/node-lsp.mjs"));

    if !native_bin.exists() {
        return Err(format!(
            "native-lsp binary not found at {}. Build with `cargo build --release --bin native-lsp`.",
            native_bin.display()
        )
        .into());
    }
    if !node_script.exists() {
        return Err(format!("missing {}", node_script.display()).into());
    }

    let fixture_dir = std::env::temp_dir().join("native-lsp-compare-fixture");
    write_fixture(&fixture_dir, file_count)?;
    let files = php_files(&fixture_dir)?;

    eprintln!(
        "fixture: {} PHP files in {}",
        files.len(),
        fixture_dir.display()
    );
    eprintln!("native:  {}", native_bin.display());
    eprintln!("node:    {} {}", node_bin, node_script.display());

    let native = measure("native-lsp", &native_bin, None, &files)?;
    let node = measure("node-lsp", Path::new(&node_bin), Some(&node_script), &files)?;

    print_table(&native, &node, files.len());

    if native.after_open_bytes >= node.after_open_bytes {
        eprintln!(
            "warning: native RSS ({}) was not below node RSS ({})",
            native_lsp::rss::format_mb(native.after_open_bytes),
            native_lsp::rss::format_mb(node.after_open_bytes)
        );
    }
    const BUDGET: u64 = 80 * 1024 * 1024;
    if native.after_open_bytes > BUDGET {
        return Err(format!(
            "native RSS {} exceeds 80 MB success bar",
            native_lsp::rss::format_mb(native.after_open_bytes)
        )
        .into());
    }
    Ok(())
}

fn default_native_bin() -> PathBuf {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(profile)
        .join("native-lsp")
}

struct Sample {
    idle_bytes: u64,
    after_open_bytes: u64,
    after_sleep_bytes: u64,
    hover_ms: u128,
    pid: u32,
}

fn measure(
    name: &str,
    program: &Path,
    script: Option<&Path>,
    files: &[PathBuf],
) -> Result<Sample, Box<dyn std::error::Error>> {
    let _ = name;
    let mut cmd = Command::new(program);
    if let Some(script) = script {
        cmd.arg(script);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let pid = child.id();
    let stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut client = LspClient {
        stdin,
        reader: BufReader::new(stdout),
        next_id: 1,
    };

    client.request(
        "initialize",
        json!({
            "processId": std::process::id(),
            "capabilities": {},
            "rootUri": null,
        }),
    )?;
    client.notify("initialized", json!({}))?;

    std::thread::sleep(Duration::from_millis(50));
    let idle = native_lsp::rss::rss_bytes_of(pid.to_string()).unwrap_or(0);

    for path in files {
        let text = std::fs::read_to_string(path)?;
        let uri = path_uri(path);
        client.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "php",
                    "version": 1,
                    "text": text
                }
            }),
        )?;
    }

    let hover_uri = path_uri(&files[0]);
    let t0 = Instant::now();
    let _hover = client.request(
        "textDocument/hover",
        json!({
            "textDocument": { "uri": hover_uri },
            "position": { "line": 4, "character": 6 }
        }),
    )?;
    let hover_ms = t0.elapsed().as_millis();

    std::thread::sleep(Duration::from_millis(80));
    let after_open = native_lsp::rss::rss_bytes_of(pid.to_string()).unwrap_or(0);

    client.notify(
        "$/nativeLsp/sleep",
        json!({ "reason": "idle", "depth": "park" }),
    )?;
    std::thread::sleep(Duration::from_millis(80));
    let after_sleep = native_lsp::rss::rss_bytes_of(pid.to_string()).unwrap_or(0);

    let _ = client.request("shutdown", json!(null));
    client.notify("exit", json!(null))?;
    drop(client);
    let _ = child.wait();

    Ok(Sample {
        idle_bytes: idle,
        after_open_bytes: after_open,
        after_sleep_bytes: after_sleep,
        hover_ms,
        pid,
    })
}

fn print_table(native: &Sample, node: &Sample, files: usize) {
    println!();
    println!("## RSS comparison ({} PHP files)", files);
    println!();
    println!("| stage | native-lsp | node-lsp | delta |");
    println!("| --- | ---: | ---: | ---: |");
    row("idle after initialize", native.idle_bytes, node.idle_bytes);
    row(
        "after didOpen all files + hover",
        native.after_open_bytes,
        node.after_open_bytes,
    );
    row(
        "after park sleep",
        native.after_sleep_bytes,
        node.after_sleep_bytes,
    );
    println!();
    println!(
        "hover latency: native {} ms, node {} ms",
        native.hover_ms, node.hover_ms
    );
    println!("pids: native {}, node {}", native.pid, node.pid);
    println!();
}

fn row(stage: &str, native: u64, node: u64) {
    let delta = if node >= native {
        format!("native saves {}", native_lsp::rss::format_mb(node - native))
    } else {
        format!(
            "native uses extra {}",
            native_lsp::rss::format_mb(native - node)
        )
    };
    println!(
        "| {stage} | {} | {} | {delta} |",
        native_lsp::rss::format_mb(native),
        native_lsp::rss::format_mb(node),
    );
}

fn path_uri(path: &Path) -> String {
    format!(
        "file://{}",
        path.canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .display()
    )
}

fn php_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) == Some("php") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn write_fixture(dir: &Path, count: usize) -> std::io::Result<()> {
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

struct LspClient {
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    next_id: i64,
}

impl LspClient {
    fn request(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let id = self.next_id;
        self.next_id += 1;
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.write(&payload)?;
        loop {
            let msg = self.read()?;
            if msg.get("id") == Some(&json!(id)) {
                if let Some(err) = msg.get("error") {
                    return Err(format!("{method} error: {err}").into());
                }
                return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), Box<dyn std::error::Error>> {
        self.write(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    fn write(&mut self, payload: &Value) -> Result<(), Box<dyn std::error::Error>> {
        let body = serde_json::to_vec(payload)?;
        write!(self.stdin, "Content-Length: {}\r\n\r\n", body.len())?;
        self.stdin.write_all(&body)?;
        self.stdin.flush()?;
        Ok(())
    }

    fn read(&mut self) -> Result<Value, Box<dyn std::error::Error>> {
        let mut content_length = None;
        let mut line = String::new();
        loop {
            line.clear();
            let n = self.reader.read_line(&mut line)?;
            if n == 0 {
                return Err("lsp stdout closed".into());
            }
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                break;
            }
            let lower = trimmed.to_ascii_lowercase();
            if let Some(rest) = lower.strip_prefix("content-length:") {
                content_length = Some(rest.trim().parse::<usize>()?);
            }
        }
        let len = content_length.ok_or("missing Content-Length")?;
        let mut buf = vec![0u8; len];
        self.reader.read_exact(&mut buf)?;
        Ok(serde_json::from_slice(&buf)?)
    }
}
