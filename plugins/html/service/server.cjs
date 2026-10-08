/** HTML owns parsing and tag policy through standard LSP and the negotiated editing 1.1 extension. */
'use strict';
const { createConnection, TextDocuments } = require('vscode-languageserver/node');
const { TextDocument } = require('vscode-languageserver-textdocument');
// The fixed package's ESM entry exposes static imports. Its default UMD factory shadows
// `require`, leaving parser-relative calls outside a single-file esbuild distribution.
const { getLanguageService } = require('vscode-html-languageservice/lib/esm/htmlLanguageService.js');
const connection = createConnection(process.stdin, process.stdout);
const documents = new TextDocuments(TextDocument);
const service = getLanguageService();
// This standard wordPattern is also understood by the host's bounded Unicode regex engine.
const wordPattern = '[A-Za-z][\\p{L}\\p{N}_:\\-.]*';
const validName = new RegExp(`^(?:${wordPattern})$`, 'u');
// These public editing 1.1 identifiers are independent of the package ID and language recognition.
const semanticCapability = 'meEditorSemanticLinkedEditing';
const semanticMethod = 'meEditor/semanticLinkedEditingRange';
let semanticLinkedEnabled = false;

/** A request always uses the synchronized document, never on-disk text or another open file. */
function target(params) {
  const document = documents.get(params.textDocument.uri);
  return document ? { document, tree: service.parseHTMLDocument(document) } : null;
}
connection.onInitialize(params => {
  const marker = params.capabilities?.experimental?.[semanticCapability];
  // A future version or unrecognized fields do not opt in to this request's version-one policy.
  semanticLinkedEnabled = !!marker && typeof marker === 'object'
    && Object.keys(marker).length === 1 && marker.version === 1;
  return { capabilities: {
  textDocumentSync: 1,
  completionProvider: { triggerCharacters: ['<', '/', ' ', '='] },
  hoverProvider: true,
  documentFormattingProvider: true,
  renameProvider: { prepareProvider: true },
  linkedEditingRangeProvider: true,
  experimental: semanticLinkedEnabled ? { [semanticCapability]: { version: 1 } } : undefined,
} };
});
connection.onCompletion(params => {
  const value = target(params);
  return value ? service.doComplete(value.document, params.position, value.tree) : null;
});
connection.onHover(params => {
  const value = target(params);
  return value ? service.doHover(value.document, params.position, value.tree) : null;
});
connection.onDocumentFormatting(params => {
  const value = target(params);
  return value ? service.format(value.document, undefined, params.options) : [];
});
connection.onPrepareRename(params => {
  const value = target(params);
  if (!value) return null;
  // Only a semantically linked tag opens rename; comments, attributes and void tags do not guess.
  const ranges = service.findLinkedEditingRanges(value.document, params.position, value.tree);
  const offset = value.document.offsetAt(params.position);
  return ranges?.find(range => value.document.offsetAt(range.start) <= offset && offset <= value.document.offsetAt(range.end)) || null;
});
connection.onRenameRequest(params => {
  const value = target(params);
  return value && validName.test(params.newName)
    ? service.doRename(value.document, params.position, params.newName, value.tree) : null;
});
/** Official parser pairing is shared, while only the negotiated extension allows unequal names. */
function linkedRanges(params, semantic) {
  const value = target(params);
  if (!value) return null;
  const ranges = service.findLinkedEditingRanges(value.document, params.position, value.tree);
  if (!ranges || ranges.length < 2) return null;
  // Standard LSP requires identical initial text/length, even for HTML's case-insensitive parser.
  const names = ranges.map(range => value.document.getText(range));
  return semantic || names.every(name => name === names[0]) ? { ranges, wordPattern } : null;
}
connection.languages.onLinkedEditingRange(params => linkedRanges(params, false));
connection.onRequest(semanticMethod, params => {
  return semanticLinkedEnabled ? linkedRanges(params, true) : null;
});
// Protocol shutdown is independent of plugin identity and leaves no application-global interpreter.
connection.onShutdown(() => null);
documents.listen(connection);
connection.listen();
