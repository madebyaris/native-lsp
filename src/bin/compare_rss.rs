//! Compare native-lsp RSS against a Node.js LSP with the same protocol surface.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use native_lsp::client::{self, LspClient};
use native_lsp::fixture::{self, OpenFile};
use native_lsp::ide::IdeReport;

fn main() {
    if let Err(err) = run() {
        eprintln!("compare-rss failed: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mode = std::env::var("COMPARE_MODE").unwrap_or_default();
    if mode.eq_ignore_ascii_case("ide") {
        return run_ide();
    }
    if mode.eq_ignore_ascii_case("tabs") {
        return run_tabs();
    }
    let mixed = mode.eq_ignore_ascii_case("mixed");
    let file_count: usize = std::env::var("COMPARE_FILES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(80);

    let native_bin = fixture::default_native_lsp();
    let node_bin = fixture::default_node_bin();
    let node_script = fixture::default_node_script();

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
        fixture::mixed_files()?
    } else {
        let fixture_dir = std::env::temp_dir().join("native-lsp-compare-fixture");
        fixture::write_php_fixtures(&fixture_dir, file_count)?;
        fixture::php_files(&fixture_dir)?
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

fn run_tabs() -> Result<(), Box<dyn std::error::Error>> {
    let native_bin = fixture::default_native_lsp();
    let node_bin = fixture::default_node_bin();
    let node_script = fixture::default_node_script();
    if !native_bin.exists() {
        return Err(format!(
            "native-lsp binary not found at {}. Build with `cargo build --release --bin native-lsp`.",
            native_bin.display()
        )
        .into());
    }
    let files = fixture::mixed_files()?;
    let native = measure_tabs("native-lsp", &native_bin, None, &files)?;
    let node = measure_tabs(
        "node-lsp",
        Path::new(&node_bin),
        Some(&node_script),
        &files,
    )?;
    println!();
    println!("## Active-tab split (one process, grammars load on demand)");
    println!();
    println!("One server. Open PHP, then JS, then the rest of `testdata/mixed/`.");
    println!("Only the latest `didOpen` stays Active (one CST). Hidden tabs nap.");
    println!("Then pin PHP active and park.");
    println!();
    println!("| stage | native RSS | CST | grammars | node RSS | delta |");
    println!("| --- | ---: | ---: | --- | ---: | --- |");
    for (n, o) in native.iter().zip(node.iter()) {
        let delta = if o.rss >= n.rss {
            format!("native saves {}", native_lsp::rss::format_mb(o.rss - n.rss))
        } else {
            format!(
                "native uses extra {}",
                native_lsp::rss::format_mb(n.rss - o.rss)
            )
        };
        println!(
            "| {} | {} | {} | {} | {} | {delta} |",
            n.stage,
            native_lsp::rss::format_mb(n.rss),
            n.cst_docs,
            if n.grammars.is_empty() {
                "—".into()
            } else {
                n.grammars.join(", ")
            },
            native_lsp::rss::format_mb(o.rss),
        );
    }
    println!();
    Ok(())
}

struct TabStage {
    stage: String,
    rss: u64,
    cst_docs: u64,
    grammars: Vec<String>,
}

fn measure_tabs(
    name: &str,
    program: &Path,
    script: Option<&Path>,
    files: &[OpenFile],
) -> Result<Vec<TabStage>, Box<dyn std::error::Error>> {
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
    std::thread::sleep(Duration::from_millis(50));

    let mut stages = Vec::new();
    stages.push(sample_tab_stage(&mut client, pid, "idle after initialize")?);

    let php = files
        .iter()
        .find(|f| f.language_id == "php")
        .ok_or("missing php fixture")?;
    open_file(&mut client, php)?;
    std::thread::sleep(Duration::from_millis(40));
    stages.push(sample_tab_stage(&mut client, pid, "open PHP (1 CST)")?);

    let js = files
        .iter()
        .find(|f| f.language_id == "javascript")
        .ok_or("missing javascript fixture")?;
    open_file(&mut client, js)?;
    std::thread::sleep(Duration::from_millis(40));
    stages.push(sample_tab_stage(
        &mut client,
        pid,
        "open JS (PHP napped, JS CST)",
    )?);

    for file in files {
        if file.language_id == "php" || file.language_id == "javascript" {
            continue;
        }
        open_file(&mut client, file)?;
    }
    std::thread::sleep(Duration::from_millis(60));
    stages.push(sample_tab_stage(
        &mut client,
        pid,
        "open remaining 8 langs (1 CST, grammars visited)",
    )?);

    let php_uri = client::path_uri(&php.path);
    for file in files {
        let uri = client::path_uri(&file.path);
        let state = if file.language_id == "php" {
            "active"
        } else {
            "hidden"
        };
        client.notify(
            "$/nativeLsp/documentVisibility",
            json!({ "uri": uri, "state": state }),
        )?;
    }
    client.notify("$/nativeLsp/wake", json!({}))?;
    let _ = client.request(
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": php_uri } }),
    )?;
    std::thread::sleep(Duration::from_millis(40));
    stages.push(sample_tab_stage(
        &mut client,
        pid,
        "pin PHP active, hide others",
    )?);

    client.notify(
        "$/nativeLsp/sleep",
        json!({ "reason": "idle", "depth": "park" }),
    )?;
    std::thread::sleep(Duration::from_millis(80));
    stages.push(sample_tab_stage(&mut client, pid, "park (drop CSTs + parsers)")?);

    let _ = client.request("shutdown", json!(null));
    client.notify("exit", json!(null))?;
    drop(client);
    let _ = child.wait();
    Ok(stages)
}

fn open_file(client: &mut LspClient, file: &OpenFile) -> Result<(), Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(&file.path)?;
    let uri = client::path_uri(&file.path);
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
    Ok(())
}

fn sample_tab_stage(
    client: &mut LspClient,
    pid: u32,
    stage: &str,
) -> Result<TabStage, Box<dyn std::error::Error>> {
    let rss = native_lsp::rss::rss_bytes_of(pid.to_string())
        .ok_or_else(|| format!("could not read RSS for pid {pid}"))?;
    let report = client.request("nativeLsp/memoryReport", json!(null))?;
    let cst_docs = report.get("cst_docs").and_then(Value::as_u64).unwrap_or(0);
    let grammars = report
        .get("grammars_loaded")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Ok(TabStage {
        stage: stage.into(),
        rss,
        cst_docs,
        grammars,
    })
}

fn run_ide() -> Result<(), Box<dyn std::error::Error>> {
    let ide_bin = fixture::default_native_ide();
    if !ide_bin.exists() {
        return Err(format!(
            "native-ide not found at {}. Build with `cargo build --release --bin native-ide`.",
            ide_bin.display()
        )
        .into());
    }
    eprintln!("ide:     {}", ide_bin.display());
    eprintln!("native:  {}", fixture::default_native_lsp().display());
    eprintln!(
        "node:    {} {}",
        fixture::default_node_bin(),
        fixture::default_node_script().display()
    );

    let native = spawn_ide_once(&ide_bin, "native")?;
    let node = spawn_ide_once(&ide_bin, "node")?;
    print_ide_table(&native, &node);

    if native.after_tabs_lsp_bytes >= node.after_tabs_lsp_bytes {
        eprintln!(
            "warning: native LSP RSS ({}) was not below node LSP RSS ({})",
            native_lsp::rss::format_mb(native.after_tabs_lsp_bytes),
            native_lsp::rss::format_mb(node.after_tabs_lsp_bytes)
        );
    }
    const BUDGET: u64 = 80 * 1024 * 1024;
    if native.after_tabs_total() > BUDGET {
        return Err(format!(
            "native-ide + native-lsp total RSS {} exceeds 80 MB success bar",
            native_lsp::rss::format_mb(native.after_tabs_total())
        )
        .into());
    }
    Ok(())
}

fn spawn_ide_once(ide_bin: &Path, lsp: &str) -> Result<IdeReport, Box<dyn std::error::Error>> {
    let output = Command::new(ide_bin)
        .args(["--lsp", lsp, "--once", "--json"])
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "native-ide --lsp {lsp} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let report: IdeReport = serde_json::from_slice(&output.stdout)?;
    Ok(report)
}

fn print_ide_table(native: &IdeReport, node: &IdeReport) {
    println!();
    println!("## Same IDE, swap the LSP (10 mixed-language tabs)");
    println!();
    println!("The editor is `native-ide` in both rows. Only the language-server child changes.");
    println!("Tab switches send `$/nativeLsp/documentVisibility` (not `didClose`).");
    println!();
    println!("| stack | IDE | LSP | total |");
    println!("| --- | ---: | ---: | ---: |");
    println!(
        "| native-ide + native-lsp | {} | {} | {} |",
        native_lsp::rss::format_mb(native.after_tabs_ide_bytes),
        native_lsp::rss::format_mb(native.after_tabs_lsp_bytes),
        native_lsp::rss::format_mb(native.after_tabs_total())
    );
    println!(
        "| native-ide + node-lsp | {} | {} | {} |",
        native_lsp::rss::format_mb(node.after_tabs_ide_bytes),
        native_lsp::rss::format_mb(node.after_tabs_lsp_bytes),
        native_lsp::rss::format_mb(node.after_tabs_total())
    );
    println!();
    println!("| stage | native total | node total | delta |");
    println!("| --- | ---: | ---: | ---: |");
    row(
        "idle (IDE buffers + LSP initialize)",
        native.idle_total(),
        node.idle_total(),
    );
    row(
        "after didOpen all tabs",
        native.after_open_total(),
        node.after_open_total(),
    );
    row(
        "after cycling tabs + hover",
        native.after_tabs_total(),
        node.after_tabs_total(),
    );
    println!();
    println!(
        "IDE RSS held roughly constant: native-host {} vs node-host {}",
        native_lsp::rss::format_mb(native.after_tabs_ide_bytes),
        native_lsp::rss::format_mb(node.after_tabs_ide_bytes)
    );
    println!(
        "hover latency: native {} ms, node {} ms",
        native.hover_ms, node.hover_ms
    );
    println!();
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
    parsers: Vec<String>,
    grammars: Vec<String>,
    cst_docs: u64,
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

    std::thread::sleep(Duration::from_millis(50));
    let idle = native_lsp::rss::rss_bytes_of(pid.to_string())
        .ok_or_else(|| format!("could not read RSS for pid {pid}"))?;

    for file in files {
        let text = std::fs::read_to_string(&file.path)?;
        let uri = client::path_uri(&file.path);
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
        let uri = client::path_uri(&file.path);
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
    let parsers = report
        .get("parsers")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let cst_docs = report.get("cst_docs").and_then(Value::as_u64).unwrap_or(0);
    let grammars = report
        .get("grammars_loaded")
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
        parsers,
        grammars,
        cst_docs,
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
        "parsers: native {} (cst docs {}, grammars {})",
        if native.parsers.is_empty() {
            "n/a".into()
        } else {
            native.parsers.join(", ")
        },
        native.cst_docs,
        if native.grammars.is_empty() {
            "—".into()
        } else {
            native.grammars.join(", ")
        }
    );
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
