//! Stdio JSON-RPC client used by the compare harness and native-ide.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{ChildStdin, ChildStdout};

use serde_json::{json, Value};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct LspClient {
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    next_id: i64,
}

impl LspClient {
    pub fn new(stdin: ChildStdin, stdout: ChildStdout) -> Self {
        Self {
            stdin,
            reader: BufReader::new(stdout),
            next_id: 1,
        }
    }

    pub fn request(&mut self, method: &str, params: Value) -> Result<Value> {
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

    pub fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.write(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    fn write(&mut self, payload: &Value) -> Result<()> {
        let body = serde_json::to_vec(payload)?;
        write!(self.stdin, "Content-Length: {}\r\n\r\n", body.len())?;
        self.stdin.write_all(&body)?;
        self.stdin.flush()?;
        Ok(())
    }

    fn read(&mut self) -> Result<Value> {
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

pub fn path_uri(path: &Path) -> String {
    format!(
        "file://{}",
        path.canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .display()
    )
}
