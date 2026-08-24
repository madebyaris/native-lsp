use std::error::Error;
use std::io::{self, BufReader, Write};
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::host::{self, HostInfo};
use crate::php;
use crate::rpc;
use crate::rss;
use crate::symbol::SymbolKind;
use crate::workspace::{Visibility, Workspace};

pub fn run() -> Result<(), Box<dyn Error + Sync + Send>> {
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let mut stdout = io::stdout().lock();

    let host = host::detect();
    let mut ws = Workspace::default();
    seed_stubs(&mut ws);
    let snapshot_path = snapshot_path();

    loop {
        let msg = match rpc::read_message(&mut reader) {
            Ok(m) => m,
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(err) => return Err(err.into()),
        };

        if let Some(id) = msg.id.clone() {
            if let Some(method) = msg.method.as_deref() {
                if method == "shutdown" {
                    let _ = ws.intern.write_snapshot(&snapshot_path);
                    rpc::write_message(&mut stdout, &rpc::ok(id, Value::Null))?;
                    continue;
                }
                match handle_request(method, msg.params.unwrap_or(Value::Null), &mut ws, &host) {
                    Ok(result) => rpc::write_message(&mut stdout, &rpc::ok(id, result))?,
                    Err(err) => {
                        rpc::write_message(&mut stdout, &rpc::err(id, -32603, err.to_string()))?
                    }
                }
            }
        } else if let Some(method) = msg.method.as_deref() {
            if method == "exit" {
                break;
            }
            handle_notification(method, msg.params.unwrap_or(Value::Null), &mut ws);
        }
    }

    let _ = stdout.flush();
    Ok(())
}

fn seed_stubs(ws: &mut Workspace) {
    for stub in php::WP_STUBS {
        ws.intern.intern(stub);
    }
}

fn snapshot_path() -> PathBuf {
    if let Ok(p) = std::env::var("NATIVE_LSP_SNAPSHOT") {
        return PathBuf::from(p);
    }
    let mut p = std::env::temp_dir();
    p.push("native-lsp");
    p.push("index.nls1");
    p
}

fn handle_request(
    method: &str,
    params: Value,
    ws: &mut Workspace,
    host: &HostInfo,
) -> Result<Value, Box<dyn Error + Sync + Send>> {
    match method {
        "initialize" => Ok(json!({
            "capabilities": {
                "textDocumentSync": 1,
                "hoverProvider": true,
                "completionProvider": { "triggerCharacters": ["$", ">", ":", "'"] },
                "documentSymbolProvider": true
            },
            "serverInfo": {
                "name": "native-lsp",
                "version": env!("CARGO_PKG_VERSION")
            },
            "nativeLsp": {
                "host": host,
                "rssBytes": rss::rss_bytes(),
                "lifecycle": true,
                "languages": crate::lang::LANGUAGE_SUPPORT
            }
        })),
        "textDocument/hover" => Ok(hover(ws, &params).unwrap_or(Value::Null)),
        "textDocument/completion" => Ok(completion(ws)),
        "textDocument/documentSymbol" => Ok(document_symbols(ws, &params)),
        "nativeLsp/memoryReport" => Ok(memory_report(ws, host)),
        _ => Err(format!("unknown method {method}").into()),
    }
}

fn handle_notification(method: &str, params: Value, ws: &mut Workspace) {
    match method {
        "initialized" => {}
        "textDocument/didOpen" => {
            if let Some(td) = params.get("textDocument") {
                let uri = td.get("uri").and_then(Value::as_str).unwrap_or_default();
                let text = td.get("text").and_then(Value::as_str).unwrap_or_default();
                let language_id = td
                    .get("languageId")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                ws.open(uri.to_string(), language_id.to_string(), text.to_string());
            }
        }
        "textDocument/didChange" => {
            let uri = params
                .pointer("/textDocument/uri")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if let Some(changes) = params.get("contentChanges").and_then(Value::as_array) {
                if let Some(text) = changes
                    .last()
                    .and_then(|c| c.get("text"))
                    .and_then(Value::as_str)
                {
                    ws.change(uri, text.to_string());
                }
            }
        }
        "textDocument/didClose" => {
            if let Some(uri) = params.pointer("/textDocument/uri").and_then(Value::as_str) {
                ws.close(uri);
            }
        }
        "$/nativeLsp/sleep" => {
            let depth = params
                .get("depth")
                .and_then(Value::as_str)
                .unwrap_or("park");
            match depth {
                "nap" => ws.nap_hidden(),
                _ => ws.park(),
            }
        }
        "$/nativeLsp/wake" => ws.wake(),
        "$/nativeLsp/documentVisibility" => {
            if let Ok(p) = serde_json::from_value::<VisibilityParams>(params) {
                let vis = match p.state.as_str() {
                    "active" => Visibility::Active,
                    "visible" => Visibility::Visible,
                    _ => Visibility::Hidden,
                };
                ws.set_visibility(&p.uri, vis);
            }
        }
        _ => {}
    }
}

#[derive(Deserialize)]
struct VisibilityParams {
    uri: String,
    state: String,
}

fn memory_report(ws: &Workspace, host: &HostInfo) -> Value {
    json!({
        "rss_bytes": rss::rss_bytes(),
        "interned": ws.intern.len(),
        "open_docs": ws.open_count(),
        "parsed_docs": ws.parsed_count(),
        "cst_docs": ws.cst_count(),
        "parsers": ws.parser_kinds(),
        "grammars_loaded": ws.grammars_loaded(),
        "languages": ws.language_ids(),
        "host": host,
    })
}

fn hover(ws: &mut Workspace, params: &Value) -> Option<Value> {
    let uri = params.pointer("/textDocument/uri")?.as_str()?;
    ws.ensure_parsed(uri);
    let line = params.pointer("/position/line")?.as_u64()? as u32;
    let doc = ws.get(uri)?;
    let symbols = doc.symbols.as_ref()?;
    let sym = crate::symbol::symbol_at_line(symbols, line)?;
    let name = ws.intern.get(sym.name_id)?;
    let display = if sym.kind == SymbolKind::Function {
        format!("{name}()")
    } else {
        name.to_string()
    };
    let mut md = format!("**{} {}** `{display}`", doc.language_id, sym.kind.label());
    if let Some(extra) = sym.extra_id.and_then(|id| ws.intern.get(id)) {
        if sym.kind == SymbolKind::Hook {
            md.push_str(&format!("\n\nWordPress hook: `{extra}`"));
        } else {
            md.push_str(&format!("\n\n{extra}"));
        }
    }
    Some(json!({
        "contents": { "kind": "markdown", "value": md }
    }))
}

fn completion(ws: &Workspace) -> Value {
    let items: Vec<Value> = ws
        .all_symbol_names()
        .into_iter()
        .take(200)
        .map(|label| {
            json!({
                "label": label,
                "kind": 3
            })
        })
        .collect();
    json!(items)
}

fn document_symbols(ws: &mut Workspace, params: &Value) -> Value {
    let Some(uri) = params.pointer("/textDocument/uri").and_then(Value::as_str) else {
        return json!([]);
    };
    ws.ensure_parsed(uri);
    let Some(doc) = ws.get(uri) else {
        return json!([]);
    };
    let Some(symbols) = &doc.symbols else {
        return json!([]);
    };
    let items: Vec<Value> = symbols
        .iter()
        .filter_map(|s| {
            let name = ws.intern.get(s.name_id)?;
            Some(json!({
                "name": name,
                "kind": s.kind.lsp_kind(),
                "detail": doc.language_id,
                "location": {
                    "uri": uri,
                    "range": {
                        "start": { "line": s.line, "character": s.character },
                        "end": { "line": s.line, "character": s.character + 1 }
                    }
                },
                "containerName": s.extra_id.and_then(|id| ws.intern.get(id))
            }))
        })
        .collect();
    json!(items)
}
