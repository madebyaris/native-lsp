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
    let mixed = std::env::var("COMPARE_MODE")
        .map(|s| s.eq_ignore_ascii_case("mixed"))
        .unwrap_or(false);
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

    let files = if mixed {
        mixed_files()?
    } else {
        let fixture_dir = std::env::temp_dir().join("native-lsp-compare-fixture");
        write_fixture(&fixture_dir, file_count)?;
        php_files(&fixture_dir)?
    };

    if files.is_empty() {
        return Err("no fixture files to open".into());
    }

    eprintln!(
        "fixture: {} files ({})",
        files.len(),
        if mixed {
            "mixed languages, one process".into()
        } else {
            format!(
                "PHP, {}",
                files[0].path.parent().unwrap_or(Path::new(".")).display()
            )
        }
    );
    for file in &files {
        if mixed {
            eprintln!(
                "  {:<12} {}",
                file.language_id,
                file.path.file_name().unwrap_or_default().to_string_lossy()
            );
        }
    }
    eprintln!("native:  {}", native_bin.display());
    eprintln!("node:    {} {}", node_bin, node_script.display());

    let native = measure("native-lsp", &native_bin, None, &files, mixed)?;
    let node = measure(
        "node-lsp",
        Path::new(&node_bin),
        Some(&node_script),
        &files,
        mixed,
    )?;

    if mixed {
        print_mixed(&native, &node);
    } else {
        print_table(&native, &node, files.len());
    }

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

struct OpenFile {
    path: PathBuf,
    language_id: String,
}

struct FileProbe {
    name: String,
    language_id: String,
    symbol_count: usize,
    hover: String,
}

struct Sample {
    idle_bytes: u64,
    after_open_bytes: u64,
    after_sleep_bytes: u64,
    hover_ms: u128,
    pid: u32,
    interned: u64,
    languages: Vec<String>,
    files: Vec<FileProbe>,
}

fn measure(
    name: &str,
    program: &Path,
    script: Option<&Path>,
    files: &[OpenFile],
    probe_each: bool,
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
    let idle = native_lsp::rss::rss_bytes_of(pid.to_string())
        .ok_or_else(|| format!("could not read RSS for pid {pid}"))?;

    for file in files {
        let text = std::fs::read_to_string(&file.path)?;
        let uri = path_uri(&file.path);
        client.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": file.language_id,
                    "version": 1,
                    "text": text
                }
            }),
        )?;
    }

    let mut probes = Vec::new();
    let mut hover_ms = 0;
    let probe_files: Vec<&OpenFile> = if probe_each {
        files.iter().collect()
    } else {
        files.iter().take(1).collect()
    };

    for file in probe_files {
        let uri = path_uri(&file.path);
        let symbols = client.request(
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": uri } }),
        )?;
        let symbol_count = symbols.as_array().map(|a| a.len()).unwrap_or(0);
        let line = symbols
            .as_array()
            .and_then(|a| a.first())
            .and_then(|s| s.pointer("/location/range/start/line"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let t0 = Instant::now();
        let hover = client.request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": 0 }
            }),
        )?;
        hover_ms += t0.elapsed().as_millis();
        let hover_text = hover
            .pointer("/contents/value")
            .and_then(Value::as_str)
            .unwrap_or("")
            .lines()
            .next()
            .unwrap_or("")
            .to_string();
        probes.push(FileProbe {
            name: file
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            language_id: file.language_id.clone(),
            symbol_count,
            hover: hover_text,
        });
    }

    std::thread::sleep(Duration::from_millis(80));
    let after_open = native_lsp::rss::rss_bytes_of(pid.to_string())
        .ok_or_else(|| format!("could not read RSS after open for pid {pid}"))?;

    let report = client.request("nativeLsp/memoryReport", json!(null))?;
    let interned = report.get("interned").and_then(Value::as_u64).unwrap_or(0);
    let languages = report
        .get("languages")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    client.notify(
        "$/nativeLsp/sleep",
        json!({ "reason": "idle", "depth": "park" }),
    )?;
    std::thread::sleep(Duration::from_millis(80));
    let after_sleep = native_lsp::rss::rss_bytes_of(pid.to_string())
        .ok_or_else(|| format!("could not read RSS after sleep for pid {pid}"))?;

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
        interned,
        languages,
        files: probes,
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

fn print_mixed(native: &Sample, node: &Sample) {
    println!();
    println!(
        "## Mixed languages ({} files, one process)",
        native.files.len()
    );
    println!();
    println!("languages: {}", native.languages.join(", "));
    println!(
        "interned names: native {}, node {}",
        native.interned, node.interned
    );
    println!();
    println!("| file | language | native symbols | native hover | node symbols | node hover |");
    println!("| --- | --- | ---: | --- | ---: | --- |");
    for (n, o) in native.files.iter().zip(node.files.iter()) {
        println!(
            "| `{}` | {} | {} | {} | {} | {} |",
            n.name,
            n.language_id,
            n.symbol_count,
            md_cell(&n.hover),
            o.symbol_count,
            md_cell(&o.hover)
        );
    }
    println!();
    println!("| stage | native-lsp | node-lsp | delta |");
    println!("| --- | ---: | ---: | ---: |");
    row("idle after initialize", native.idle_bytes, node.idle_bytes);
    row(
        "after didOpen 10 languages + hover",
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
        "hover latency (all files): native {} ms, node {} ms",
        native.hover_ms, node.hover_ms
    );
    println!("pids: native {}, node {}", native.pid, node.pid);
    println!();
}

fn md_cell(s: &str) -> String {
    if s.is_empty() {
        return "_none_".into();
    }
    format!("`{}`", s.replace('|', "\\|").replace('`', "'"))
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

fn php_files(dir: &Path) -> std::io::Result<Vec<OpenFile>> {
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

fn mixed_files() -> Result<Vec<OpenFile>, Box<dyn std::error::Error>> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/mixed");
    if !dir.is_dir() {
        return Err(format!("missing mixed fixtures at {}", dir.display()).into());
    }
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let language_id = native_lsp::lang::infer_from_uri(&format!(
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
