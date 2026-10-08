/** JavaScript's standard formatter is an independent role backed by the official TypeScript engine. */
'use strict';
const { createConnection, TextDocuments } = require('vscode-languageserver/node');
const { TextDocument } = require('vscode-languageserver-textdocument');
const ts = require('typescript');
const connection = createConnection(process.stdin, process.stdout);
const documents = new TextDocuments(TextDocument);
connection.onInitialize(() => ({ capabilities: { textDocumentSync: 1, documentFormattingProvider: true } }));

/** Formatting owns only a synchronized syntax snapshot; it never starts project build tools. */
connection.onDocumentFormatting(params => {
  const document = documents.get(params.textDocument.uri);
  if (!document) return [];
  const filename = 'document.jsx';
  const source = document.getText();
  const host = {
    getScriptFileNames: () => [filename],
    getScriptVersion: () => String(document.version),
    getScriptSnapshot: name => name === filename ? ts.ScriptSnapshot.fromString(source) : undefined,
    getCurrentDirectory: () => '/',
    getCompilationSettings: () => ({ allowJs: true, jsx: ts.JsxEmit.Preserve }),
    getDefaultLibFileName: () => '',
    fileExists: name => name === filename,
    readFile: name => name === filename ? source : undefined,
  };
  const service = ts.createLanguageService(host);
  try {
    const options = { ...ts.getDefaultFormatCodeSettings(),
      indentSize: params.options.tabSize, tabSize: params.options.tabSize,
      convertTabsToSpaces: params.options.insertSpaces, newLineCharacter: '\n' };
    return service.getFormattingEditsForDocument(filename, options).map(edit => ({
      range: { start: document.positionAt(edit.span.start), end: document.positionAt(edit.span.start + edit.span.length) },
      newText: edit.newText,
    }));
  } finally { service.dispose(); }
});
connection.onShutdown(() => null);
documents.listen(connection);
connection.listen();
