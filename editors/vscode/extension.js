const vscode = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

let client;

function command() {
  const kind = process.env.NATIVE_LSP_KIND || "native";
  const root = process.env.NATIVE_LSP_ROOT || process.cwd();
  if (kind === "node") {
    return {
      command: process.env.NODE_BIN || "node",
      args: [`${root}/compare/node-lsp.mjs`],
      transport: TransportKind.stdio,
    };
  }
  return {
    command: process.env.NATIVE_LSP_BIN || `${root}/target/release/native-lsp`,
    args: [],
    transport: TransportKind.stdio,
  };
}

async function activate(context) {
  client = new LanguageClient("nativeLsp", "native-lsp", command(), {
    documentSelector: [{ scheme: "file" }],
    outputChannelName: "native-lsp",
  });
  await client.start();
  context.subscriptions.push({
    dispose: () => {
      if (client) {
        client.stop();
      }
    },
  });
}

async function deactivate() {
  if (client) {
    await client.stop();
  }
}

module.exports = { activate, deactivate };
