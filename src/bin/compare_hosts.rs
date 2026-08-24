//! Drive Neovim, Emacs, Helix, VS Code, and native-ide with the same LSP A/B.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use native_lsp::fixture;
use native_lsp::rss;

fn main() {
    if let Err(err) = run() {
        eprintln!("compare-hosts failed: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let native_bin = fixture::default_native_lsp();
    if !native_bin.exists() {
        return Err(format!(
            "native-lsp not found at {}. Build with cargo build --release --bin native-lsp.",
            native_bin.display()
        )
        .into());
    }

    reap_stray_lsps();
    eprintln!("root:    {}", root.display());
    eprintln!("native:  {}", native_bin.display());
    eprintln!("nvim:    {}", which("nvim").display());
    eprintln!("emacs:   {}", which("emacs").display());
    eprintln!("hx:      {}", which("hx").display());
    eprintln!("code:    {}", which("code").display());

    let mut rows = Vec::new();
    let mut vscode_dumps = Vec::new();
    for host in [
        Host::NativeIde,
        Host::Neovim,
        Host::Emacs,
        Host::Helix,
        Host::VsCode,
    ] {
        if !host_wanted(host) {
            continue;
        }
        if !host.available() {
            eprintln!("skip {}: binary not on PATH", host.name());
            continue;
        }
        for kind in ["native", "node"] {
            eprintln!("--- {} + {}-lsp ---", host.name(), kind);
            match measure(host, kind, &root, &native_bin) {
                Ok(measured) => {
                    eprintln!(
                        "  IDE {}  LSP {}  total {}",
                        rss::format_mb(measured.row.ide_bytes),
                        measured
                            .row
                            .lsp_bytes
                            .map(rss::format_mb)
                            .unwrap_or_else(|| "n/a".into()),
                        rss::format_mb(measured.row.total()),
                    );
                    if let Some(dump) = measured.vscode {
                        vscode_dumps.push(dump);
                    }
                    rows.push(measured.row);
                }
                Err(err) => eprintln!("  failed: {err}"),
            }
        }
    }

    println!();
    println!("## Same mixed files, real editors, swap only the LSP");
    println!();
    println!("Each editor opens `testdata/mixed/` (10 languages). IDE RSS is the editor");
    println!("process tree. LSP RSS is the language-server child.");
    println!();
    println!("| host | LSP | IDE | LSP | total | hover files |");
    println!("| --- | --- | ---: | ---: | ---: | ---: |");
    for row in &rows {
        println!(
            "| {} | {} | {} | {} | {} | {} |",
            row.host,
            row.server,
            rss::format_mb(row.ide_bytes),
            row.lsp_bytes
                .map(rss::format_mb)
                .unwrap_or_else(|| "_n/a_".into()),
            rss::format_mb(row.total()),
            row.hover_files
        );
    }
    println!();
    if !vscode_dumps.is_empty() {
        println!("## Why VS Code looks huge (it is not native-lsp)");
        println!();
        println!("`compare-hosts` sums every process whose cmdline contains the unique");
        println!("`--user-data-dir` marker. That tree is Chromium main + renderer + GPU");
        println!("+ crashpad + the **Node extension host**. native-lsp only replaces the");
        println!("language-server child. Opening ten files still loads the full workbench.");
        println!();
        for dump in &vscode_dumps {
            println!("### vscode + {}-lsp", dump.server);
            println!();
            print!("{}", rss::format_role_totals(&dump.procs));
            println!();
            print!("{}", rss::format_proc_table(&dump.procs));
            println!();
        }
    }
    Ok(())
}

fn host_wanted(host: Host) -> bool {
    let Ok(raw) = std::env::var("COMPARE_HOSTS") else {
        return true;
    };
    if raw.trim().is_empty() {
        return true;
    }
    raw.split(',')
        .any(|s| s.trim().eq_ignore_ascii_case(host.name()))
}

#[derive(Clone, Copy)]
enum Host {
    NativeIde,
    Neovim,
    Emacs,
    Helix,
    VsCode,
}

impl Host {
    fn name(self) -> &'static str {
        match self {
            Self::NativeIde => "native-ide",
            Self::Neovim => "neovim",
            Self::Emacs => "emacs",
            Self::Helix => "helix",
            Self::VsCode => "vscode",
        }
    }

    fn available(self) -> bool {
        match self {
            Self::NativeIde => fixture::default_native_ide().exists(),
            Self::Neovim => which("nvim").exists(),
            Self::Emacs => which("emacs").exists(),
            Self::Helix => which("hx").exists(),
            Self::VsCode => which("code").exists(),
        }
    }
}

struct Row {
    host: String,
    server: String,
    ide_bytes: u64,
    lsp_bytes: Option<u64>,
    hover_files: usize,
}

struct VsCodeDump {
    server: String,
    procs: Vec<rss::ProcSample>,
}

struct Measured {
    row: Row,
    vscode: Option<VsCodeDump>,
}

impl Row {
    fn total(&self) -> u64 {
        self.ide_bytes.saturating_add(self.lsp_bytes.unwrap_or(0))
    }
}

fn measure(
    host: Host,
    kind: &str,
    root: &Path,
    native_bin: &Path,
) -> Result<Measured, Box<dyn std::error::Error>> {
    match host {
        Host::NativeIde => wrap_row(measure_native_ide(kind, root)?),
        Host::Neovim => wrap_row(measure_nvim(kind, root, native_bin)?),
        Host::Emacs => wrap_row(measure_emacs(kind, root, native_bin)?),
        Host::Helix => wrap_row(measure_helix(kind, root, native_bin)?),
        Host::VsCode => measure_vscode(kind, root, native_bin),
    }
}

fn wrap_row(row: Row) -> Result<Measured, Box<dyn std::error::Error>> {
    Ok(Measured { row, vscode: None })
}

fn measure_native_ide(kind: &str, root: &Path) -> Result<Row, Box<dyn std::error::Error>> {
    let ide = fixture::default_native_ide();
    let output = Command::new(&ide)
        .current_dir(root)
        .args(["--lsp", kind, "--once", "--json"])
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "native-ide failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let report: native_lsp::ide::IdeReport = serde_json::from_slice(&output.stdout)?;
    Ok(Row {
        host: "native-ide".into(),
        server: kind.into(),
        ide_bytes: report.after_tabs_ide_bytes,
        lsp_bytes: Some(report.after_tabs_lsp_bytes),
        hover_files: report.tabs.iter().filter(|t| !t.hover.is_empty()).count(),
    })
}

fn measure_nvim(
    kind: &str,
    root: &Path,
    native_bin: &Path,
) -> Result<Row, Box<dyn std::error::Error>> {
    let dir = unique_dir("nvim");
    let report_path = dir.join("report.json");
    let done_path = dir.join("done");
    let init = root.join("editors/nvim/init.lua");
    let mut child = Command::new("nvim")
        .current_dir(root)
        .args(["-u"])
        .arg(&init)
        .args(["-i", "NONE", "--headless", "--noplugin"])
        .env("NATIVE_LSP_ROOT", root)
        .env("NATIVE_LSP_KIND", kind)
        .env("NATIVE_LSP_BIN", native_bin)
        .env("NATIVE_LSP_REPORT", &report_path)
        .env("NATIVE_LSP_DONE", &done_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    wait_for_file(&report_path, Duration::from_secs(20)).map_err(|err| {
        let _ = child.kill();
        err
    })?;
    thread::sleep(Duration::from_millis(200));
    let row = sample_host("neovim", kind, child.id(), &report_path)?;
    let _ = fs::write(&done_path, b"ok");
    wait_child(&mut child, Duration::from_secs(8))?;
    Ok(row)
}

fn measure_emacs(
    kind: &str,
    root: &Path,
    native_bin: &Path,
) -> Result<Row, Box<dyn std::error::Error>> {
    let dir = unique_dir("emacs");
    let report_path = dir.join("report.json");
    let done_path = dir.join("done");
    let script = root.join("editors/emacs/native-lsp.el");
    let mut child = Command::new("emacs")
        .current_dir(root)
        .args(["--batch", "-Q", "-l"])
        .arg(&script)
        .env("NATIVE_LSP_ROOT", root)
        .env("NATIVE_LSP_KIND", kind)
        .env("NATIVE_LSP_BIN", native_bin)
        .env("NATIVE_LSP_REPORT", &report_path)
        .env("NATIVE_LSP_DONE", &done_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    wait_for_file(&report_path, Duration::from_secs(25)).map_err(|err| {
        let _ = child.kill();
        err
    })?;
    thread::sleep(Duration::from_millis(200));
    let row = sample_host("emacs", kind, child.id(), &report_path)?;
    let _ = fs::write(&done_path, b"ok");
    wait_child(&mut child, Duration::from_secs(8))?;
    Ok(row)
}

fn measure_helix(
    kind: &str,
    root: &Path,
    native_bin: &Path,
) -> Result<Row, Box<dyn std::error::Error>> {
    let dir = unique_dir("helix");
    let xdg = dir.join("xdg");
    let cfg = xdg.join("helix");
    fs::create_dir_all(&cfg)?;
    write_helix_config(&cfg, kind, root, native_bin)?;
    let files = fixture::mixed_files()?;
    let mut cmd = Command::new("script");
    cmd.arg("-q")
        .arg("-c")
        .arg(helix_command(root, &files))
        .arg(dir.join("typescript"))
        .current_dir(root)
        .env("XDG_CONFIG_HOME", &xdg)
        .env("HELIX_RUNTIME", helix_runtime())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = cmd.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(12);
    let mut lsp_pid = None;
    while Instant::now() < deadline {
        if let Some(pid) = rss::find_lsp_pid(child.id()) {
            lsp_pid = Some(pid);
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }
    let (ide, lsp) = sample_split(child.id());
    let _ = lsp_pid;
    let _ = child.kill();
    let _ = child.wait();
    Ok(Row {
        host: "helix".into(),
        server: kind.into(),
        ide_bytes: ide,
        lsp_bytes: lsp,
        hover_files: 0,
    })
}

fn measure_vscode(
    kind: &str,
    root: &Path,
    native_bin: &Path,
) -> Result<Measured, Box<dyn std::error::Error>> {
    let dir = unique_dir("vscode");
    let user = dir.join("user");
    let ext = dir.join("ext");
    install_vscode_extension(&ext, root)?;
    let settings_dir = user.join("User");
    fs::create_dir_all(&settings_dir)?;
    let settings = if kind == "node" {
        serde_json::json!({
            "nativeLsp.command": [fixture::default_node_bin(), root.join("compare/node-lsp.mjs").display().to_string()],
            "nativeLsp.root": root,
        })
    } else {
        serde_json::json!({
            "nativeLsp.command": [native_bin.display().to_string()],
            "nativeLsp.root": root,
        })
    };
    fs::write(
        settings_dir.join("settings.json"),
        serde_json::to_vec_pretty(&settings)?,
    )?;
    let marker = dir.file_name().unwrap().to_string_lossy().into_owned();
    let files = fixture::mixed_files()?;
    let mut args = vec![
        "--disable-gpu".into(),
        "--disable-workspace-trust".into(),
        "--new-window".into(),
        format!("--user-data-dir={}", user.display()),
        format!("--extensions-dir={}", ext.display()),
    ];
    for file in &files {
        args.push(file.path.to_string_lossy().into_owned());
    }
    let mut child = Command::new("code")
        .current_dir(root)
        .args(&args)
        .env(
            "DISPLAY",
            std::env::var("DISPLAY").unwrap_or_else(|_| ":1".into()),
        )
        .env("NATIVE_LSP_ROOT", root)
        .env("NATIVE_LSP_KIND", kind)
        .env("NATIVE_LSP_BIN", native_bin)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let mut ide_pids = Vec::new();
    let mut lsp_pid: Option<u32>;
    let deadline = Instant::now() + Duration::from_secs(25);
    while Instant::now() < deadline {
        ide_pids = rss::pids_with_cmdline(&marker);
        lsp_pid = ide_pids.iter().copied().find_map(rss::find_lsp_pid);
        if !ide_pids.is_empty() && lsp_pid.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(400));
    }
    // Electron keeps spawning renderer / extensionHost after the first window.
    thread::sleep(Duration::from_secs(5));
    ide_pids = rss::pids_with_cmdline(&marker);
    lsp_pid = ide_pids.iter().copied().find_map(rss::find_lsp_pid);
    let ide = ide_pids
        .iter()
        .copied()
        .filter(|p| !rss::is_lsp_pid(*p))
        .filter_map(|p| rss::rss_bytes_of(p.to_string()))
        .sum();
    let lsp = lsp_pid
        .filter(|p| rss::is_lsp_pid(*p))
        .and_then(|p| rss::rss_bytes_of(p.to_string()));
    let mut dump_pids = ide_pids.clone();
    if let Some(pid) = lsp_pid.filter(|p| rss::is_lsp_pid(*p)) {
        if !dump_pids.contains(&pid) {
            dump_pids.push(pid);
        }
    }
    let procs = rss::sample_procs(&dump_pids);
    let _ = child.kill();
    let _ = child.wait();
    // Electron often detaches; kill the user-data-dir process tree.
    for pid in rss::pids_with_cmdline(&marker) {
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
    Ok(Measured {
        row: Row {
            host: "vscode".into(),
            server: kind.into(),
            ide_bytes: ide,
            lsp_bytes: lsp,
            hover_files: 0,
        },
        vscode: Some(VsCodeDump {
            server: kind.into(),
            procs,
        }),
    })
}

fn sample_host(
    host: &str,
    kind: &str,
    pid: u32,
    report_path: &Path,
) -> Result<Row, Box<dyn std::error::Error>> {
    let (ide, lsp) = sample_split(pid);
    Ok(Row {
        host: host.into(),
        server: kind.into(),
        ide_bytes: ide,
        lsp_bytes: lsp,
        hover_files: hover_count(report_path),
    })
}

fn sample_split(pid: u32) -> (u64, Option<u64>) {
    let lsp = rss::find_lsp_pid(pid);
    let mut exclude = Vec::new();
    if let Some(lsp_pid) = lsp {
        exclude.push(lsp_pid);
        exclude.extend(rss::descendants(lsp_pid));
    }
    let mut pids = rss::descendants(pid);
    pids.push(pid);
    let ide = pids
        .into_iter()
        .filter(|p| !exclude.contains(p))
        .filter_map(|p| rss::rss_bytes_of(p.to_string()))
        .sum();
    let lsp_bytes = lsp.and_then(|p| rss::rss_bytes_of(p.to_string()));
    (ide, lsp_bytes)
}

fn hover_count(path: &Path) -> usize {
    let Ok(text) = fs::read_to_string(path) else {
        return 0;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return 0;
    };
    v.get("probes")
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|p| {
                    p.get("hover")
                        .and_then(|h| h.as_str())
                        .map(|s| !s.is_empty())
                        .unwrap_or(false)
                })
                .count()
        })
        .unwrap_or(0)
}

fn wait_for_file(path: &Path, timeout: Duration) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if path.exists() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(format!("timed out waiting for {}", path.display()).into())
}

fn wait_child(child: &mut Child, timeout: Duration) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Some(status) = child.try_wait()? {
            if status.success() || status.code() == Some(0) {
                return Ok(());
            }
            return Err(format!("host exited {status}").into());
        }
        thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

fn reap_stray_lsps() {
    for pid in rss::pids_with_cmdline("node-lsp.mjs")
        .into_iter()
        .chain(rss::pids_with_cmdline("/native-lsp"))
    {
        let name = rss::comm(pid);
        if name == "native-lsp" || name == "node" {
            let _ = Command::new("kill").arg(pid.to_string()).status();
        }
    }
}

fn unique_dir(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "native-lsp-{}-{}",
        prefix,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    let _ = fs::create_dir_all(&dir);
    dir
}

fn which(name: &str) -> PathBuf {
    Command::new("which")
        .arg(name)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| PathBuf::from(s.trim()))
        .unwrap_or_else(|| PathBuf::from(name))
}

fn helix_runtime() -> PathBuf {
    let opt = PathBuf::from("/opt/editors/helix/runtime");
    if opt.exists() {
        opt
    } else {
        PathBuf::from("runtime")
    }
}

fn helix_command(root: &Path, files: &[native_lsp::fixture::OpenFile]) -> String {
    let mut cmd = String::from("hx");
    for file in files {
        cmd.push(' ');
        cmd.push_str(&file.path.display().to_string());
    }
    let _ = root;
    cmd
}

fn write_helix_config(
    cfg: &Path,
    kind: &str,
    root: &Path,
    native_bin: &Path,
) -> std::io::Result<()> {
    fs::write(
        cfg.join("config.toml"),
        "[editor.lsp]\nenable = true\ndisplay-messages = true\n",
    )?;
    let (command, args) = if kind == "node" {
        (
            "node".to_string(),
            format!(
                "args = [\"{}\"]\n",
                root.join("compare/node-lsp.mjs").display()
            ),
        )
    } else {
        (native_bin.display().to_string(), String::new())
    };
    let langs = [
        "php",
        "javascript",
        "typescript",
        "html",
        "css",
        "json",
        "yaml",
        "sql",
        "python",
        "rust",
    ];
    let mut toml = format!("[language-server.native-lsp]\ncommand = \"{command}\"\n{args}\n");
    for lang in langs {
        toml.push_str(&format!(
            "[[language]]\nname = \"{lang}\"\nlanguage-servers = [\"native-lsp\"]\n\n"
        ));
    }
    fs::write(cfg.join("languages.toml"), toml)
}

fn install_vscode_extension(ext_dir: &Path, root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let src = root.join("editors/vscode");
    let dest = ext_dir.join("native-lsp.native-lsp-host-0.1.0");
    fs::create_dir_all(ext_dir)?;
    if dest.exists() {
        fs::remove_dir_all(&dest)?;
    }
    copy_dir(&src, &dest)?;
    if !dest.join("node_modules/vscode-languageclient").exists() {
        let status = Command::new("npm")
            .args(["install", "--omit=dev", "--no-fund", "--no-audit"])
            .current_dir(&dest)
            .status()?;
        if !status.success() {
            return Err("npm install vscode-languageclient failed".into());
        }
    }
    Ok(())
}

fn copy_dir(src: &Path, dest: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let to = dest.join(entry.file_name());
        if entry.path().is_dir() {
            if entry.file_name() == "node_modules" {
                continue;
            }
            copy_dir(&entry.path(), &to)?;
        } else {
            fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}
