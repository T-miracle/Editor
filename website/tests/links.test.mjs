// Link checks: every link a reader can click must resolve.
//
// Scope is deliberately limited to what the repository controls. Internal links
// are resolved against the page tree with the same base prefix the build applies,
// and anchors are checked against the headings of the target page. External URLs
// are not fetched: a network blip would fail the suite for reasons the author
// cannot fix, so their presence is reported but not enforced.

import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, it } from 'node:test';
import { basePrefix, collectLinks, listPages, siteRoot } from './helpers.mjs';

const pages = listPages();

/** Route of a page as the built site exposes it, including the base prefix. */
const routeOf = (page) => `${basePrefix}/${page.locale}/${page.trail}/`.replace(/\/index\/$/, '/');

const routes = new Set(pages.map(routeOf));
const pageByRoute = new Map(pages.map((page) => [routeOf(page), page]));

/**
 * Heading slugs Astro generates: lower case, punctuation dropped, spaces hyphenated.
 *
 * @param {string} heading raw heading text
 */
const slugify = (heading) =>
  heading
    .toLowerCase()
    .replace(/[`*_]/g, '')
    .replace(/[^\p{L}\p{N}\s-]/gu, '')
    .trim()
    .replace(/\s+/g, '-');

/** Heading anchors declared by a page body, including duplicates Astro suffixes. */
function anchorsOf(page) {
  const anchors = new Set();
  const seen = new Map();
  for (const line of page.body.split(/\r?\n/)) {
    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    if (heading === null) continue;
    const slug = slugify(heading[2]);
    const count = seen.get(slug) ?? 0;
    seen.set(slug, count + 1);
    anchors.add(count === 0 ? slug : `${slug}-${count}`);
  }
  return anchors;
}

const isExternal = (url) => /^(https?:|mailto:)/.test(url);

describe('documentation links', () => {
  it('contains at least one internal link, so a broken parser fails loudly', () => {
    const internal = pages.flatMap((page) =>
      collectLinks(page).filter((link) => !isExternal(link.url)),
    );
    assert.ok(internal.length > 0, 'no internal links found; link collection is probably broken');
  });

  it('resolves every internal link to a published page', () => {
    const failures = [];
    for (const page of pages) {
      for (const link of collectLinks(page)) {
        if (isExternal(link.url) || link.url.startsWith('#')) continue;
        const target = `${basePrefix}${link.url.startsWith('/') ? link.url : `/${link.url}`}`;
        if (!routes.has(target)) {
          failures.push(`${page.id}:${link.line} -> ${link.url}`);
        }
      }
    }
    assert.deepEqual(failures, [], 'internal links must resolve to a page in the built site');
  });

  it('resolves anchors against the target page headings', () => {
    const failures = [];
    for (const page of pages) {
      for (const link of collectLinks(page)) {
        const hash = link.url.indexOf('#');
        if (isExternal(link.url) || hash === -1) continue;

        const path = link.url.slice(0, hash);
        const anchor = link.url.slice(hash + 1);
        const targetRoute = path === '' ? routeOf(page) : `${basePrefix}${path}`;
        const target = pageByRoute.get(targetRoute);
        if (target === undefined) {
          failures.push(`${page.id}:${link.line} -> ${link.url} (page not found)`);
          continue;
        }
        if (!anchorsOf(target).has(anchor)) {
          failures.push(`${page.id}:${link.line} -> ${link.url} (missing anchor)`);
        }
      }
    }
    assert.deepEqual(failures, [], 'anchors must match a heading of the target page');
  });

  it('uses site-root paths for internal links instead of hard-coded base prefixes', () => {
    const failures = [];
    for (const page of pages) {
      for (const link of collectLinks(page)) {
        if (isExternal(link.url)) continue;
        if (link.url.startsWith(`${basePrefix}/`)) {
          failures.push(`${page.id}:${link.line} -> ${link.url} (drop the base prefix)`);
        }
      }
    }
    assert.deepEqual(
      failures,
      [],
      'Markdown links are written without the base prefix; the build adds it',
    );
  });

  it('reports external links without failing the suite', () => {
    const external = pages.flatMap((page) =>
      collectLinks(page)
        .filter((link) => isExternal(link.url))
        .map((link) => `${page.id}:${link.line} -> ${link.url}`),
    );
    // No assertion: external availability is not under this repository's control.
    console.log(`external links referenced: ${external.length}`);
  });

  it('publishes an entry point at the site root', () => {
    // Every page lives under a language prefix, so the project URL has nothing to
    // serve without this file and answers 404 even though the whole site is
    // published. The redirect is a build artifact rather than a page, so it is
    // checked here instead of through the content collection.
    const entry = join(siteRoot, 'public', 'index.html');
    assert.ok(existsSync(entry), 'public/index.html must exist to serve the site root');
    const html = readFileSync(entry, 'utf8');
    for (const locale of ['en', 'zh-cn']) {
      assert.ok(
        html.includes(`${basePrefix}/${locale}/`),
        `the root entry point must link to the ${locale} tree`,
      );
    }
  });
});
