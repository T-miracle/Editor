// Shared parsing helpers for the documentation tests.
//
// The tests read the Markdown sources directly instead of the built HTML: the
// checks are about what authors wrote (page sets, declared counterparts, link
// targets), and reading sources keeps failures pointing at a file and a line
// rather than at generated output. Link targets are resolved with the same base
// prefixing rule the build applies, so a link that validates here renders
// correctly in `dist`.

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';

/** Absolute path of the site project. */
export const siteRoot = resolve(import.meta.dirname, '..');

/** Absolute path of the bilingual Markdown tree. */
export const contentRoot = join(siteRoot, 'src', 'content', 'docs');

/** Locales the site publishes; mirrors the Astro i18n configuration. */
export const locales = ['en', 'zh-cn'];

/** Prefix applied to absolute Markdown links, mirroring `base` in the Astro config. */
export const basePrefix = '/Editor';

/** Frontmatter fields every page must declare. */
export const requiredFields = ['title', 'description', 'section', 'order', 'alternate'];

/** Sections a page may belong to. */
export const sections = ['guide', 'sdk'];

/**
 * True when a page is declared but not yet written.
 *
 * Draft pages exist so an owed translation stays visible to the page-set and
 * counterpart checks without ever reaching a reader: the build skips them.
 *
 * @param {{ fields: Record<string, string> }} page parsed page
 */
export function isDraft(page) {
  return page.fields.draft === 'true';
}

/**
 * Read the frontmatter block of a Markdown file.
 *
 * Only flat `key: value` pairs are supported, which is all the schema uses; a
 * richer parser would be a dependency the site does not need.
 *
 * @param {string} text full file contents
 * @returns {{ fields: Record<string, string>, body: string, bodyStartLine: number }}
 */
export function parseFrontmatter(text) {
  const match = /^---\r?\n([\s\S]*?)\r?\n---[ \t]*\r?\n?/.exec(text);
  if (match === null) return { fields: {}, body: text, bodyStartLine: 1 };

  const fields = {};
  const lines = match[1].split(/\r?\n/);
  for (const line of lines) {
    const pair = /^([A-Za-z][\w-]*):\s*(.*)$/.exec(line);
    if (pair === null) continue;
    fields[pair[1]] = pair[2].trim().replace(/^["']|["']$/g, '');
  }
  // The body starts after the opening fence, the field lines and the closing fence.
  return { fields, body: text.slice(match[0].length), bodyStartLine: lines.length + 3 };
}

/**
 * List every Markdown page under the content tree.
 *
 * @returns {Array<{ id: string, locale: string, section: string, slug: string,
 *   trail: string, path: string, fields: Record<string, string>, body: string,
 *   bodyStartLine: number }>}
 */
export function listPages() {
  const pages = [];
  const walk = (directory) => {
    for (const entry of readdirSync(directory)) {
      const full = join(directory, entry);
      if (statSync(full).isDirectory()) {
        walk(full);
        continue;
      }
      if (!entry.endsWith('.md')) continue;

      const id = relative(contentRoot, full).replace(/\\/g, '/').replace(/\.md$/, '');
      const segments = id.split('/');
      const text = readFileSync(full, 'utf8');
      const parsed = parseFrontmatter(text);
      pages.push({
        id,
        locale: segments[0],
        section: segments.length > 2 ? segments[1] : '',
        slug: segments[segments.length - 1],
        trail: segments.slice(1).join('/'),
        path: full,
        /** Raw file contents, so a test can inspect the frontmatter verbatim. */
        text,
        ...parsed,
      });
    }
  };
  walk(contentRoot);
  return pages;
}

/**
 * Collect inline and image link targets with their source line.
 *
 * @param {{ body: string, bodyStartLine: number }} page
 * @returns {Array<{ url: string, line: number }>}
 */
export function collectLinks(page) {
  const links = [];
  const pattern = /!?\[[^\]]*\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g;
  const lines = page.body.split(/\r?\n/);
  lines.forEach((text, index) => {
    for (const match of text.matchAll(pattern)) {
      links.push({ url: match[1], line: page.bodyStartLine + index });
    }
  });
  return links;
}

/**
 * Site paths that a link may legitimately point at.
 *
 * Astro emits directory-style routes, so `/<locale>/<trail>/` is the canonical
 * form; the locale and section roots are included because they are real pages.
 *
 * @param {ReturnType<typeof listPages>} pages
 * @returns {Set<string>}
 */
export function routeSet(pages) {
  const routes = new Set();
  for (const page of pages) {
    routes.add(`/${page.locale}/${page.trail}/`.replace(/\/index\/$/, '/'));
  }
  return routes;
}
