// Starts `lxir lsp` for .lxir files. The language client is an npm
// dependency: without `npm install` in this folder the extension stays
// declarative (highlighting and snippets) and says how to turn it on.
const vscode = require('vscode');

let client;

function activate(context) {
  const cfg = vscode.workspace.getConfiguration('lxir');
  if (!cfg.get('server.enable', true)) {
    return;
  }
  let lc;
  try {
    lc = require('vscode-languageclient/node');
  } catch (e) {
    vscode.window.showInformationMessage(
      'lxir: the language server needs the language client — run `npm install` in the extension folder.'
    );
    return;
  }
  const server = { command: cfg.get('server.path', 'lxir'), args: cfg.get('server.args', ['lsp']) };
  client = new lc.LanguageClient('lxir', 'lxir', server, {
    documentSelector: [{ scheme: 'file', language: 'lxir' }],
  });
  client.start();
}

function deactivate() {
  return client ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
