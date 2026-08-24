#!/usr/bin/env node
/**
 * Minimal Node.js LSP used only for RSS comparison.
 * Same protocol surface as native-lsp: initialize, document sync, hover,
 * completion, documentSymbol, park sleep. Stores documents as heap objects.
 */
import { Buffer } from "node:buffer";

const docs = new Map();
let internCount = 0;

function identAt(s) {
  const m = s.trimStart().match(/^[A-Za-z_\\][A-Za-z0-9_\\]*/);
  return m ? m[0] : null;
}

function firstQuoted(s) {
  const t = s.trimStart();
  const q = t[0];
  if (q !== "'" && q !== '"') return null;
  const end = t.indexOf(q, 1);
  if (end < 0) return null;
  return t.slice(1, end);
}

function extractPhp(text) {
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
      symbols.push({ name: m[1], kind: "hook", line: i, extra: m[3] });
    }
  }
  return symbols;
}

function stripExport(s) {
  return s.replace(/^export\s+(default\s+)?/, "");
}

function extractJs(text, typescript) {
  const symbols = [];
  const lines = text.split(/\n/);
  for (let i = 0; i < lines.length; i++) {
    const trimmed = lines[i].trimStart();
    if (trimmed.startsWith("//") || trimmed.startsWith("*") || trimmed.startsWith("/*")) {
      continue;
    }
    const rest = stripExport(trimmed);
    if (typescript) {
      for (const kw of ["interface ", "type ", "enum "]) {
        if (rest.startsWith(kw)) {
          const name = identAt(rest.slice(kw.length));
          if (name) symbols.push({ name, kind: "type", line: i });
        }
      }
    }
    if (rest.startsWith("class ")) {
      const name = identAt(rest.slice(6));
      if (name) symbols.push({ name, kind: "class", line: i });
    }
    const fn = rest.replace(/^async\s+/, "");
    if (fn.startsWith("function ")) {
      const name = identAt(fn.slice(9));
      if (name) symbols.push({ name, kind: "function", line: i });
    }
    const constMatch = rest.match(/^(?:const|let|var)\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(async|function|\()/);
    if (constMatch) {
      symbols.push({ name: constMatch[1], kind: "function", line: i });
    }
  }
  return symbols;
}

function extractHtml(text) {
  const symbols = [];
  const lines = text.split(/\n/);
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const idRe = /\bid=["']([^"']+)["']/g;
    let m;
    while ((m = idRe.exec(line))) {
      symbols.push({ name: m[1], kind: "tag", line: i, extra: "id" });
    }
    const classRe = /\bclass=["']([^"']+)["']/g;
    while ((m = classRe.exec(line))) {
      for (const c of m[1].split(/\s+/).filter(Boolean)) {
        symbols.push({ name: c, kind: "rule", line: i, extra: "class" });
      }
    }
    const tagRe = /<([a-zA-Z][a-zA-Z0-9-]*)/g;
    while ((m = tagRe.exec(line))) {
      if (m[1].includes("-")) {
        symbols.push({ name: m[1], kind: "tag", line: i });
      }
    }
  }
  return symbols;
}

function extractCss(text) {
  const symbols = [];
  const lines = text.split(/\n/);
  for (let i = 0; i < lines.length; i++) {
    const trimmed = lines[i].trim();
    if (!trimmed || trimmed.startsWith("/*") || trimmed.startsWith("*")) continue;
    if (trimmed.startsWith("@keyframes ")) {
      const name = trimmed.slice(11).split(/[\s{]/)[0];
      if (name) symbols.push({ name, kind: "rule", line: i });
      continue;
    }
    if (trimmed.startsWith("@")) continue;
    const before = trimmed.split("{")[0];
    for (const sel of before.split(",")) {
      const t = sel.trim();
      const name = t.replace(/^[.#]/, "").split(/[\s:>+~[({]/)[0];
      if (name) symbols.push({ name, kind: "rule", line: i });
    }
  }
  return symbols;
}

function extractJson(text) {
  const symbols = [];
  const lines = text.split(/\n/);
  for (let i = 0; i < lines.length; i++) {
    const m = lines[i].trimStart().match(/^"([^"]+)"\s*:/);
    if (m) symbols.push({ name: m[1], kind: "key", line: i });
  }
  return symbols;
}

function extractYaml(text) {
  const symbols = [];
  const lines = text.split(/\n/);
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (line.startsWith("#") || !line.trim() || line.trim() === "---") continue;
    const indent = line.length - line.trimStart().length;
    if (indent > 2) continue;
    const trimmed = line.trimStart();
    if (trimmed.startsWith("-")) continue;
    const colon = trimmed.indexOf(":");
    if (colon <= 0) continue;
    const key = trimmed.slice(0, colon).trim();
    if (key && !key.includes(" ")) {
      symbols.push({ name: key, kind: "key", line: i });
    }
  }
  return symbols;
}

function extractSql(text) {
  const symbols = [];
  const lines = text.split(/\n/);
  for (let i = 0; i < lines.length; i++) {
    const trimmed = lines[i].trimStart();
    if (trimmed.startsWith("--")) continue;
    const upper = trimmed.toUpperCase();
    const kws = [
      "CREATE TABLE ",
      "CREATE VIEW ",
      "CREATE INDEX ",
      "CREATE UNIQUE INDEX ",
      "CREATE FUNCTION ",
      "CREATE PROCEDURE ",
    ];
    for (const kw of kws) {
      const idx = upper.indexOf(kw);
      if (idx < 0) continue;
      let after = trimmed.slice(idx + kw.length).trimStart();
      after = after.replace(/^IF NOT EXISTS\s+/i, "");
      const name = firstQuoted(after) || identAt(after);
      if (name) symbols.push({ name, kind: "table", line: i });
      break;
    }
  }
  return symbols;
}

function extractPython(text) {
  const symbols = [];
  const lines = text.split(/\n/);
  for (let i = 0; i < lines.length; i++) {
    const trimmed = lines[i].trimStart();
    if (trimmed.startsWith("#")) continue;
    if (trimmed.startsWith("class ")) {
      const name = identAt(trimmed.slice(6));
      if (name) symbols.push({ name, kind: "class", line: i });
    }
    const def = trimmed.replace(/^async\s+/, "");
    if (def.startsWith("def ")) {
      const name = identAt(def.slice(4));
      if (name) symbols.push({ name, kind: "function", line: i });
    }
  }
  return symbols;
}

function extractRust(text) {
  const symbols = [];
  const lines = text.split(/\n/);
  for (let i = 0; i < lines.length; i++) {
    let rest = lines[i].trimStart();
    if (rest.startsWith("//")) continue;
    rest = rest.replace(/^pub(\([^)]+\))?\s+/, "").replace(/^async\s+/, "");
    const kinds = [
      ["fn ", "function"],
      ["struct ", "class"],
      ["enum ", "type"],
      ["trait ", "type"],
      ["mod ", "module"],
      ["type ", "type"],
      ["const ", "variable"],
      ["impl ", "class"],
      ["macro_rules! ", "function"],
    ];
    for (const [kw, kind] of kinds) {
      if (rest.startsWith(kw)) {
        const name = identAt(rest.slice(kw.length));
        if (name) symbols.push({ name, kind, line: i });
      }
    }
  }
  return symbols;
}

function extract(text, languageId) {
  let symbols;
  switch (languageId) {
    case "php":
      symbols = extractPhp(text);
      break;
    case "javascript":
      symbols = extractJs(text, false);
      break;
    case "typescript":
      symbols = extractJs(text, true);
      break;
    case "html":
      symbols = extractHtml(text);
      break;
    case "css":
      symbols = extractCss(text);
      break;
    case "json":
      symbols = extractJson(text);
      break;
    case "yaml":
      symbols = extractYaml(text);
      break;
    case "sql":
      symbols = extractSql(text);
      break;
    case "python":
      symbols = extractPython(text);
      break;
    case "rust":
      symbols = extractRust(text);
      break;
    default:
      symbols = extractPhp(text);
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

function lspKind(kind) {
  switch (kind) {
    case "class":
      return 5;
    case "function":
      return 12;
    case "hook":
      return 24;
    case "type":
      return 11;
    case "variable":
      return 13;
    case "tag":
      return 5;
    case "rule":
      return 7;
    case "key":
      return 8;
    case "table":
      return 23;
    case "module":
      return 2;
    default:
      return 13;
  }
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
      const sym = [...(doc?.symbols || [])].reverse().find((s) => s.line === line);
      const label = sym?.kind || "";
      const display = label === "function" ? `${sym.name}()` : sym?.name;
      respond(
        msg.id,
        sym
          ? {
              contents: {
                kind: "markdown",
                value: `**${doc.languageId} ${label}** \`${display}\``,
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
          kind: lspKind(s.kind),
          detail: doc.languageId,
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
        languages: [...new Set([...docs.values()].map((d) => d.languageId))].sort(),
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
        languageId: td.languageId || "php",
        lines: td.text.split(/\n/),
        symbols: extract(td.text, td.languageId || "php"),
      });
      return;
    }
    case "textDocument/didChange": {
      const uri = msg.params.textDocument.uri;
      const text = msg.params.contentChanges.at(-1)?.text;
      if (text == null) return;
      const prev = docs.get(uri);
      const languageId = prev?.languageId || "php";
      docs.set(uri, {
        text,
        languageId,
        lines: text.split(/\n/),
        symbols: extract(text, languageId),
      });
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
        if (!doc.symbols) doc.symbols = extract(doc.text, doc.languageId || "php");
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
