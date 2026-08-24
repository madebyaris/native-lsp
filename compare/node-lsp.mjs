#!/usr/bin/env node
/**
 * Minimal Node.js LSP used only for RSS comparison.
 * Same protocol surface as native-lsp: initialize, document sync, hover,
 * completion, documentSymbol, park sleep. Stores documents as heap objects.
 */
import { Buffer } from "node:buffer";

const docs = new Map();
let internCount = 0;

function extract(text) {
  const symbols = [];
  const lines = text.split(/\n/);
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const classMatch = line.match(/\bclass\s+([A-Za-z_\\][A-Za-z0-9_\\]*)/);
    if (classMatch) {
      symbols.push({ name: classMatch[1], kind: "class", line: i });
    }
    const fnMatch = line.match(
      /\b(?:public|private|protected|static)?\s*function\s+([A-Za-z_][A-Za-z0-9_]*)/,
    );
    if (fnMatch) {
      symbols.push({ name: fnMatch[1], kind: "function", line: i });
    }
    const hookRe = /\b(add_action|add_filter)\(\s*(['"])([^'"]+)\2/g;
    let m;
    while ((m = hookRe.exec(line))) {
      symbols.push({ name: m[1], kind: "hook", line: i, hook: m[3] });
    }
  }
  internCount += symbols.length;
  return symbols;
}

const STUBS = [
  "WP_Query",
  "WP_Post",
  "get_option",
  "add_action",
  "add_filter",
  "wp_enqueue_script",
];

function send(msg) {
  const body = Buffer.from(JSON.stringify(msg), "utf8");
  process.stdout.write(`Content-Length: ${body.length}\r\n\r\n`);
  process.stdout.write(body);
}

function respond(id, result) {
  send({ jsonrpc: "2.0", id, result });
}

function handleRequest(msg) {
  switch (msg.method) {
    case "initialize":
      respond(msg.id, {
        capabilities: {
          textDocumentSync: 1,
          hoverProvider: true,
          completionProvider: { triggerCharacters: ["$", ">"] },
          documentSymbolProvider: true,
        },
        serverInfo: { name: "node-lsp-compare", version: "0.1.0" },
      });
      return;
    case "shutdown":
      respond(msg.id, null);
      return;
    case "textDocument/hover": {
      const uri = msg.params.textDocument.uri;
      const line = msg.params.position.line;
      const doc = docs.get(uri);
      const sym = doc?.symbols?.find((s) => s.line === line);
      respond(
        msg.id,
        sym
          ? {
              contents: {
                kind: "markdown",
                value: `**${sym.kind}** \`${sym.name}\``,
              },
            }
          : null,
      );
      return;
    }
    case "textDocument/completion": {
      const names = new Set(STUBS);
      for (const doc of docs.values()) {
        for (const s of doc.symbols || []) names.add(s.name);
      }
      respond(
        msg.id,
        [...names].slice(0, 200).map((label) => ({
          label,
          kind: 3,
        })),
      );
      return;
    }
    case "textDocument/documentSymbol": {
      const doc = docs.get(msg.params.textDocument.uri);
      respond(
        msg.id,
        (doc?.symbols || []).map((s) => ({
          name: s.name,
          kind: s.kind === "class" ? 5 : 12,
          location: {
            uri: msg.params.textDocument.uri,
            range: {
              start: { line: s.line, character: 0 },
              end: { line: s.line, character: 1 },
            },
          },
        })),
      );
      return;
    }
    case "nativeLsp/memoryReport":
      respond(msg.id, {
        rss_bytes: process.memoryUsage().rss,
        interned: internCount,
        open_docs: docs.size,
        parsed_docs: [...docs.values()].filter((d) => d.symbols).length,
        host: { profile: "node", io: "heap", os: process.platform },
      });
      return;
    default:
      send({
        jsonrpc: "2.0",
        id: msg.id,
        error: { code: -32601, message: `unknown method ${msg.method}` },
      });
  }
}

function handleNotification(msg) {
  switch (msg.method) {
    case "initialized":
    case "exit":
      if (msg.method === "exit") process.exit(0);
      return;
    case "textDocument/didOpen": {
      const td = msg.params.textDocument;
      docs.set(td.uri, {
        text: td.text,
        lines: td.text.split(/\n/),
        symbols: extract(td.text),
      });
      return;
    }
    case "textDocument/didChange": {
      const uri = msg.params.textDocument.uri;
      const text = msg.params.contentChanges.at(-1)?.text;
      if (text == null) return;
      docs.set(uri, { text, lines: text.split(/\n/), symbols: extract(text) });
      return;
    }
    case "textDocument/didClose":
      docs.delete(msg.params.textDocument.uri);
      return;
    case "$/nativeLsp/sleep":
      for (const doc of docs.values()) {
        doc.symbols = null;
      }
      return;
    case "$/nativeLsp/wake":
      for (const doc of docs.values()) {
        if (!doc.symbols) doc.symbols = extract(doc.text);
      }
      return;
    default:
  }
}

let buf = Buffer.alloc(0);

function onBytes(chunk) {
  buf = Buffer.concat([buf, chunk]);
  while (true) {
    const headerEnd = buf.indexOf("\r\n\r\n");
    if (headerEnd < 0) return;
    const header = buf.subarray(0, headerEnd).toString("utf8");
    const match = header.match(/Content-Length:\s*(\d+)/i);
    if (!match) {
      buf = buf.subarray(headerEnd + 4);
      continue;
    }
    const len = Number(match[1]);
    const total = headerEnd + 4 + len;
    if (buf.length < total) return;
    const body = buf.subarray(headerEnd + 4, total).toString("utf8");
    buf = buf.subarray(total);
    const msg = JSON.parse(body);
    if (Object.prototype.hasOwnProperty.call(msg, "id")) {
      handleRequest(msg);
    } else {
      handleNotification(msg);
    }
  }
}

process.stdin.on("data", onBytes);
process.stdin.on("end", () => process.exit(0));
