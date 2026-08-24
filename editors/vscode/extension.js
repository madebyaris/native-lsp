const fs = require("fs");
const path = require("path");
const vscode = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

const LANGUAGES = [
  "php",
  "javascript",
  "javascriptreact",
  "typescript",
  "typescriptreact",
  "html",
  "css",
  "json",
  "yaml",
  "sql",
  "python",
  "rust",
];

let client;
let logPath;

function log(msg) {
  const line = `[native-lsp] ${new Date().toISOString()} ${msg}\n`;
  if (logPath) {
    try {
      fs.appendFileSync(logPath, line);
    } catch (_) {
      // ignore
    }
  }
}

function cfg() {
  return vscode.workspace.getConfiguration("nativeLsp");
}

function serverCommand() {
  const fromSettings = cfg().get("command");
  if (Array.isArray(fromSettings) && fromSettings.length > 0) {
    return {
      command: fromSettings[0],
      args: fromSettings.slice(1),
      transport: TransportKind.stdio,
    };
  }
  const kind = process.env.NATIVE_LSP_KIND || "native";
  const root =
    cfg().get("root") || process.env.NATIVE_LSP_ROOT || vscode.workspace.rootPath || process.cwd();
  if (kind === "node") {
    return {
      command: process.env.NODE_BIN || "node",
      args: [path.join(root, "compare/node-lsp.mjs")],
      transport: TransportKind.stdio,
    };
  }
  return {
    command: process.env.NATIVE_LSP_BIN || path.join(root, "target/release/native-lsp"),
    args: [],
    transport: TransportKind.stdio,
  };
}

function documentSelector() {
  return LANGUAGES.map((language) => ({ scheme: "file", language }));
}

function sendVisibility() {
  if (!client) {
    return;
  }
  const active = vscode.window.activeTextEditor
    ? vscode.window.activeTextEditor.document.uri.toString()
    : "";
  for (const doc of vscode.workspace.textDocuments) {
    if (doc.uri.scheme !== "file") {
      continue;
    }
    const state = doc.uri.toString() === active ? "active" : "hidden";
    client.sendNotification("$/nativeLsp/documentVisibility", {
      uri: doc.uri.toString(),
      state,
    });
  }
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function symbolPosition(symbols) {
  const first = Array.isArray(symbols) ? symbols[0] : undefined;
  if (!first) {
    return new vscode.Position(0, 0);
  }
  const range =
    first.selectionRange ||
    (first.location && first.location.range) ||
    first.range;
  if (range && range.start) {
    return range.start;
  }
  return new vscode.Position(0, 0);
}

function hoverText(hovers) {
  if (!hovers || !hovers[0] || !hovers[0].contents) {
    return "";
  }
  const first = hovers[0].contents[0];
  if (typeof first === "string") {
    return first.split("\n")[0];
  }
  if (first && first.value) {
    return String(first.value).split("\n")[0];
  }
  return "";
}

async function execute(command, ...args) {
  try {
    return await vscode.commands.executeCommand(command, ...args);
  } catch (err) {
    log(`${command} failed: ${err}`);
    return undefined;
  }
}

async function waitForWorkspaceFiles() {
  const include = "**/*.{php,js,ts,html,css,json,yml,yaml,sql,py,rs}";
  const start = Date.now();
  while (Date.now() - start < 25000) {
    const folders = vscode.workspace.workspaceFolders;
    if (folders && folders.length) {
      const uris = await vscode.workspace.findFiles(include, "**/node_modules/**");
      if (uris.length >= 8) {
        return uris.sort((a, b) => a.fsPath.localeCompare(b.fsPath));
      }
    }
    await sleep(250);
  }
  return vscode.workspace.findFiles(include, "**/node_modules/**");
}

async function waitForSymbols(uri) {
  const start = Date.now();
  let symbols = [];
  while (Date.now() - start < 20000) {
    symbols = (await execute("vscode.executeDocumentSymbolProvider", uri)) || [];
    if (Array.isArray(symbols) && symbols.length > 0) {
      return symbols;
    }
    await sleep(400);
  }
  return Array.isArray(symbols) ? symbols : [];
}

async function probeWorkspace() {
  const reportPath = cfg().get("reportPath") || process.env.NATIVE_LSP_REPORT;
  if (!reportPath) {
    return;
  }
  log(`probe start kind=${process.env.NATIVE_LSP_KIND || "native"} report=${reportPath}`);
  const uris = await waitForWorkspaceFiles();
  log(`found ${uris.length} workspace files`);
  const probes = [];
  for (let i = 0; i < uris.length; i++) {
    const uri = uris[i];
    const doc = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(doc, { preview: false, preserveFocus: false });
    sendVisibility();
    if (i === 0) {
      await waitForSymbols(uri);
    }
    const symbols = (await execute("vscode.executeDocumentSymbolProvider", uri)) || [];
    const symbolCount = Array.isArray(symbols) ? symbols.length : 0;
    const pos = symbolPosition(symbols);
    let hovers = (await execute("vscode.executeHoverProvider", uri, pos)) || [];
    if (!hoverText(hovers)) {
      await sleep(500);
      hovers = (await execute("vscode.executeHoverProvider", uri, pos)) || [];
    }
    const completions = await execute("vscode.executeCompletionItemProvider", uri, pos);
    const completionCount = completions && completions.items ? completions.items.length : 0;
    probes.push({
      name: path.basename(uri.fsPath),
      path: vscode.workspace.asRelativePath(uri),
      languageId: doc.languageId,
      symbolCount,
      hover: hoverText(hovers),
      completionCount,
    });
    log(`${path.basename(uri.fsPath)} symbols=${symbolCount} hover=${hoverText(hovers).slice(0, 80)}`);
  }
  const payload = {
    host: "vscode",
    server: process.env.NATIVE_LSP_KIND || "native",
    probes,
  };
  fs.mkdirSync(path.dirname(reportPath), { recursive: true });
  fs.writeFileSync(reportPath, JSON.stringify(payload, null, 2));
  log("probe wrote report");
}

async function activate(context) {
  logPath = cfg().get("logPath") || process.env.NATIVE_LSP_LOG || "";
  const enabled = cfg().get("enable") !== false && process.env.NATIVE_LSP_KIND !== "stock";
  log(`activate enabled=${enabled} kind=${process.env.NATIVE_LSP_KIND || ""}`);
  if (enabled) {
    client = new LanguageClient("nativeLsp", "native-lsp", serverCommand(), {
      documentSelector: documentSelector(),
      outputChannelName: "native-lsp",
      synchronize: {
        fileEvents: vscode.workspace.createFileSystemWatcher(
          "**/*.{php,js,jsx,ts,tsx,html,css,json,yml,yaml,sql,py,rs}"
        ),
      },
    });
    await client.start();
    context.subscriptions.push(
      vscode.window.onDidChangeActiveTextEditor(() => sendVisibility()),
      {
        dispose: () => {
          if (client) {
            client.stop();
          }
        },
      }
    );
    sendVisibility();
    log("language client ready");
  }
  if (cfg().get("reportPath") || process.env.NATIVE_LSP_REPORT) {
    try {
      await probeWorkspace();
    } catch (err) {
      const reportPath = cfg().get("reportPath") || process.env.NATIVE_LSP_REPORT;
      log(`probe failed: ${err}`);
      fs.writeFileSync(
        reportPath,
        JSON.stringify({ host: "vscode", error: String(err), probes: [] }, null, 2)
      );
    }
  }
}

async function deactivate() {
  if (client) {
    await client.stop();
  }
}

module.exports = { activate, deactivate };
