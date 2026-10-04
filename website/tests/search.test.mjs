// Search checks: the built index must answer reader queries in both languages.
//
// The site ships a static index rather than a search service, so the risk is not a
// failed request but an index that silently misses a tree: Pagefind splits results
// per language, and a missing attribute or an unindexed page would leave one group
// of readers with an empty box and no error anywhere.
//
// The check runs a search the way a reader does. Pagefind's runtime is a browser ES
// module that spawns a Web Worker, so this serves the build output over HTTP, imports
// the shipped bundle as a module (Node rejects `import.meta.url` inside eval) and
// runs its worker in-process because Node has no Web Worker.
import assert from 'node:assert/strict';
import { copyFileSync, existsSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { extname, join, normalize } from 'node:path';
import { describe, it } from 'node:test';
import { siteRoot } from './helpers.mjs';

const dist = join(siteRoot, 'dist');
const BASE = '/Editor/';

/** Content types the runtime needs to fetch its shards and instantiate WebAssembly. */
const CONTENT_TYPES = {
  '.html': 'text/html',
  '.js': 'text/javascript',
  '.mjs': 'text/javascript',
  '.css': 'text/css',
  '.json': 'application/json',
  '.wasm': 'application/wasm',
  '.pf_index': 'application/octet-stream',
  '.pf_fragment': 'application/octet-stream',
  '.pf_meta': 'application/octet-stream',
};

/**
 * Node has no Web Worker; run the shipped worker script in-process.
 *
 * Search still goes through Pagefind's own worker code and its own index files, so
 * the query path under test is the one a browser uses.
 */
function installWorkerShim() {
  globalThis.Worker = class {
    #workers = [];
    #handlers = [];

    constructor(url) {
      this.url = url.toString();
      this.onmessage = null;
      const source = readFileSync(new URL(this.url), 'utf8');
      const self = {
        onmessage: null,
        postMessage: (data) => {
          const event = { data };
          this.onmessage?.(event);
          for (const handler of this.#handlers) handler(event);
        },
        addEventListener: (type, handler) => {
          if (type === 'message') this.#handlers.push(handler);
        },
        importScripts: () => {},
        location: { href: this.url },
        fetch: (...args) => fetch(...args),
        TextDecoder,
        TextEncoder,
        WebAssembly,
        console,
      };
      new Function('self', 'globalThis', `${source}\n//# sourceURL=${this.url}`)(self, self);
      this.self = self;
      void this.#workers;
    }

    postMessage(data) {
      this.self.onmessage?.({ data });
    }

    addEventListener(type, handler) {
      if (type === 'message') this.#handlers.push(handler);
    }

    terminate() {}
  };
}

/** Serve the build output, because the runtime fetches its shards over HTTP. */
function startServer() {
  const server = createServer((request, response) => {
    const path = decodeURIComponent(new URL(request.url, 'http://localhost').pathname);
    if (!path.startsWith(BASE)) {
      response.writeHead(404).end('outside base');
      return;
    }
    let file = join(dist, normalize(path.slice(BASE.length)));
    if (!existsSync(file) || extname(file) === '') file = join(file, 'index.html');
    if (!existsSync(file)) {
      response.writeHead(404).end('not found');
      return;
    }
    response.writeHead(200, { 'content-type': CONTENT_TYPES[extname(file)] ?? 'text/plain' });
    response.end(readFileSync(file));
  });
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => resolve({ server, port: server.address().port }));
  });
}

/** Import the shipped runtime so `import.meta.url` resolves to the bundle directory. */
async function loadRuntime() {
  const source = join(dist, 'pagefind', 'pagefind.js');
  const copy = join(dist, 'pagefind', 'pagefind.test.mjs');
  copyFileSync(source, copy);
  return { module: await import(`file://${copy}`), copy };
}

/** Search several terms in one initialisation and return the result URLs per term. */
async function searchTerms(terms, language, baseUrl, port) {
  const { module, copy } = await loadRuntime();
  try {
    // The shipped runtime is a set of named exports that initialise one instance:
    // a second `init` in the same process keeps the first instance's state and
    // quietly returns nothing. Every query in this file therefore runs in one
    // initialisation, which is why both languages are checked together below.
    await module.options({ basePath: baseUrl });
    await module.init(language);
    const found = {};
    for (const term of terms) {
      const result = await module.search(term);
      const pages = await Promise.all(result.results.slice(0, 5).map((item) => item.data()));
      found[term] = pages.map((page) => page.url.replace(`http://127.0.0.1:${port}/Editor`, ''));
    }
    return found;
  } finally {
    rmSync(copy, { force: true });
  }
}

describe('site search', () => {
  // The index is a build product and is not committed, so a fresh checkout has no
  // `dist` until the build runs. `npm test` must still pass there, which is why both
  // checks below are conditional on the artifact: the build runs them for real.
  const entryPath = join(dist, 'pagefind', 'pagefind-entry.json');

  it('builds one index covering every published page', (t) => {
    if (!existsSync(entryPath)) {
      t.skip('no build output yet; run npm run build');
      return;
    }
    const entry = JSON.parse(readFileSync(entryPath, 'utf8'));
    const languages = Object.keys(entry.languages ?? {});
    // One index is expected on purpose: Pagefind needs a CJK segmenter extension to
    // index Chinese text, and without it a Chinese search returns nothing while
    // English keeps working. The build forces a single language so Chinese prose is
    // segmented, and both trees are then reachable through the same index.
    assert.equal(
      languages.length,
      1,
      `the index must be built with one forced language, found ${languages.join(', ')}`,
    );
    const total = Object.values(entry.languages).reduce((sum, item) => sum + (item.page_count ?? 0), 0);
    assert.ok(total >= 34, `the index must cover the published pages, covers ${total}`);
  });

  it('answers English and Chinese queries from their own trees', async (t) => {
    if (!existsSync(entryPath)) {
      t.skip('no build output yet; run npm run build');
      return;
    }
    installWorkerShim();
    const { server, port } = await startServer();
    try {
      const language = Object.keys(
        JSON.parse(readFileSync(join(dist, 'pagefind', 'pagefind-entry.json'), 'utf8')).languages,
      )[0];
      // `fold` is English prose; `编辑` is the Chinese word for editing and appears
      // only in the Chinese tree, so it returns nothing at all unless that text was
      // segmented while indexing. That is the regression this check catches.
      const found = await searchTerms(
        ['fold', '编辑'],
        language,
        `http://127.0.0.1:${port}${BASE}pagefind/`,
        port,
      );

      assert.ok(found.fold.length > 0, 'a documented English term must return results');
      assert.ok(
        found.fold.some((url) => url.includes('/en/')),
        `English results must include the English tree, got ${found.fold.join(', ')}`,
      );

      assert.ok(found['编辑'].length > 0, 'a documented Chinese term must return results');
      assert.ok(
        found['编辑'].some((url) => url.includes('/zh-cn/')),
        `Chinese results must include the Chinese tree, got ${found['编辑'].join(', ')}`,
      );
    } finally {
      server.close();
    }
  });
});
