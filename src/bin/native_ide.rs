//! Native workbench: one editor process, one LSP child, mixed-language tabs.

use std::env;
use std::io::IsTerminal;
use std::path::PathBuf;

use native_lsp::ide::{self, LspKind};

fn main() {
    if let Err(err) = run() {
        eprintln!("native-ide failed: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut kind = LspKind::Native;
    let mut once = false;
    let mut json = false;
    let mut dir: Option<PathBuf> = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--lsp" => {
                let value = args.next().ok_or("--lsp needs native or node")?;
                kind = LspKind::parse(&value).ok_or_else(|| format!("unknown --lsp {value}"))?;
            }
            "--once" => once = true,
            "--json" => {
                json = true;
                once = true;
            }
            "--dir" => dir = Some(PathBuf::from(args.next().ok_or("--dir needs a path")?)),
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            other => return Err(format!("unknown arg {other}").into()),
        }
    }

    if !once && !std::io::stdin().is_terminal() {
        once = true;
    }

    let files = ide::files_from_dir(dir.as_deref())?;
    if once {
        let report = ide::run_once(kind, files)?;
        if json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            ide::print_report(&report);
        }
    } else {
        ide::run_repl(kind, files)?;
    }
    Ok(())
}

fn print_help() {
    eprintln!(
        "\
native-ide — tiny native workbench for a fair LSP A/B

  native-ide [--lsp native|node] [--once] [--json] [--dir testdata/mixed]

Same editor, swap only the language server. Reports IDE RSS, LSP RSS, and
total. Tab switches send $/nativeLsp/documentVisibility (not didClose).

  --once   open mixed files, cycle tabs, print a report, exit
  --json   --once, JSON on stdout (stderr stays human)
  --lsp    native (default) or node
"
    );
}
