// Starts `loxl lsp` for .loxl files. The language client is an npm
// dependency: without `npm install` in this folder the extension stays
// declarative (highlighting and snippets) and says how to turn it on.
const vscode = require('vscode');

let client;

function activate(context) {
  const cfg = vscode.workspace.getConfiguration('loxl');
  if (!cfg.get('server.enable', true)) {
    return;
  }
  let lc;
  try {
    lc = require('vscode-languageclient/node');
  } catch (e) {
    vscode.window.showInformationMessage(
      'Lox Logic: the language server needs the language client — run `npm install` in the extension folder.'
    );
    return;
  }
  // The program to run comes from user settings only: a workspace's
  // .vscode/settings.json must not choose an executable.
  const user = (key, fallback) => {
    const v = cfg.inspect(key);
    return v.globalValue !== undefined ? v.globalValue : fallback;
  };
  const server = { command: user('server.path', 'loxl'), args: user('server.args', ['lsp']) };
  client = new lc.LanguageClient('loxl', 'Lox Logic', server, {
    documentSelector: [{ scheme: 'file', language: 'loxl' }],
  });
  client.start();
}

function deactivate() {
  return client ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
