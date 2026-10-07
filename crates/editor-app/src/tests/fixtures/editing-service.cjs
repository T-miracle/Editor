/** A stdio-only test plugin service exposes observable requests and configurable latency. */
const fs = require('node:fs');
const [log, delay, mode] = process.argv.slice(2);
const documents = new Map();
// DocumentChanges proposals carry the exact version last synchronized through this real transport.
const versions = new Map();
let input = Buffer.alloc(0);

/** Write exactly one JSON-RPC frame, allowing delayed responses without blocking other messages. */
function reply(id, result) {
  fs.appendFileSync(log, JSON.stringify({ direction: 'response', id, result }) + '\n');
  const body = Buffer.from(JSON.stringify({ jsonrpc: '2.0', id, result }));
  process.stdout.write(`Content-Length: ${body.length}\r\n\r\n`);
  process.stdout.write(body);
}

/** LSP positions count UTF-16 code units; the fixture deliberately includes emoji in its documents. */
function position(source, offset) {
  const lines = source.slice(0, offset).split('\n');
  return { line: lines.length - 1, character: lines.at(-1).length };
}

/** Pair only the fixture plugin's element; the host still obtains all semantics via the public LSP. */
function pairs(source, point) {
  const lines = source.split('\n');
  const cursor = lines.slice(0, point.line).reduce((total, line) => total + line.length + 1, 0) + point.character;
  const open = /<([\p{L}_][\p{L}\p{N}_:\-.]*)>/u.exec(source);
  if (!open) return null;
  const close = source.indexOf(`</${open[1]}>`, open.index + open[0].length);
  if (close < 0) return null;
  const offsets = [open.index + 1, close + 2];
  if (!offsets.some(start => start <= cursor && cursor <= start + open[1].length)) return null;
  return { ranges: offsets.map(start => ({ start: position(source, start), end: position(source, start + open[1].length) })), wordPattern: '[\\p{L}_][\\p{L}\\p{N}_:\\-.]*' };
}

/** Each recorded request is evidence for one selected formatter and ordered native editing. */
function handle(message) {
  fs.appendFileSync(log, JSON.stringify(message) + '\n');
  const { method, params, id } = message;
  if (method === 'textDocument/didOpen') documents.set(params.textDocument.uri, params.textDocument.text);
  if (method === 'textDocument/didChange') documents.set(params.textDocument.uri, params.contentChanges.at(-1).text);
  if (method === 'textDocument/didOpen' || method === 'textDocument/didChange') versions.set(params.textDocument.uri, params.textDocument.version);
  if (method === 'textDocument/didClose') {
    documents.delete(params.textDocument.uri);
    versions.delete(params.textDocument.uri);
  }
  if (id === undefined) return;
  if (method === 'initialize') return reply(id, { capabilities: { textDocumentSync: 1, completionProvider: {}, hoverProvider: true, definitionProvider: true, documentFormattingProvider: true, renameProvider: { prepareProvider: true }, linkedEditingRangeProvider: true } });
  if (method === 'shutdown') return reply(id, null);
  const source = documents.get(params?.textDocument?.uri) || '';
  let result = null;
  if (method === 'textDocument/linkedEditingRange') result = pairs(source, params.position);
  if (method === 'textDocument/prepareRename') result = pairs(source, params.position)?.ranges[0] || null;
  if (method === 'textDocument/rename') {
    const linked = pairs(source, params.position);
    result = linked && { changes: { [params.textDocument.uri]: linked.ranges.map(range => ({ range, newText: params.newName })) } };
    // The public versioned form is used by actual servers such as the fixed XML distribution.
    if (linked && mode === 'versioned') {
      result = { documentChanges: [{ textDocument: { uri: params.textDocument.uri, version: versions.get(params.textDocument.uri) }, edits: linked.ranges.map(range => ({ range, newText: params.newName })) }] };
    }
  }
  if (method === 'textDocument/completion') result = [{ label: 'analysis-kept' }];
  if (method === 'textDocument/hover') result = { contents: 'analysis-kept' };
  if (method === 'textDocument/definition') result = [];
  if (method === 'textDocument/formatting') {
    const range = { start: { line: 0, character: 0 }, end: position(source, source.length) };
    result = [{ range, newText: `/* ${mode} */\n${source}` }];
    // A valid first edit cannot be applied when a later proposal overlaps it.
    if (mode === 'invalid') result.push({ range, newText: 'invalid partial result' });
  }
  setTimeout(() => reply(id, result), Number(delay));
}

/** Parse byte-framed input incrementally, including several messages received in one pipe read. */
process.stdin.on('data', chunk => {
  input = Buffer.concat([input, chunk]);
  for (;;) {
    const boundary = input.indexOf('\r\n\r\n');
    if (boundary < 0) return;
    const match = /Content-Length:\s*(\d+)/i.exec(input.subarray(0, boundary).toString());
    if (!match) process.exit(2);
    const length = Number(match[1]);
    if (input.length < boundary + 4 + length) return;
    const body = input.subarray(boundary + 4, boundary + 4 + length);
    input = input.subarray(boundary + 4 + length);
    handle(JSON.parse(body));
  }
});
