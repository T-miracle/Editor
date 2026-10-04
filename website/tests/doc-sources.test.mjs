// Document source checks: the page tree must be complete, mirrored and declared.
//
// Two failures this test exists to catch. First, a half-finished translation:
// the English tree gains a page and the Chinese tree does not, so a reader who
// switches language lands on a 404. Second, a page whose `alternate` points at
// the wrong counterpart, which silently breaks the language switcher even
// though both pages exist.
//
// The checks are deliberately structural. They cannot tell whether a translation
// is accurate, only that the two trees correspond, so translation quality stays
// a review responsibility.

import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { describe, it } from 'node:test';
import {
  contentRoot,
  isDraft,
  listPages,
  locales,
  requiredFields,
  sections,
} from './helpers.mjs';

const pages = listPages();

/** Pages keyed by their path inside a locale, e.g. `guide/index`. */
const byLocale = new Map(locales.map((locale) => [locale, new Map()]));
for (const page of pages) {
  byLocale.get(page.locale)?.set(page.trail, page);
}

/** Ticket-defined route: the site must not publish an empty placeholder page. */
const MIN_BODY_LENGTH = 200;

describe('documentation sources', () => {
  it('finds at least one page per locale, so a broken walk fails loudly', () => {
    for (const locale of locales) {
      assert.ok(
        (byLocale.get(locale)?.size ?? 0) > 0,
        `no pages found for locale ${locale} under ${contentRoot}`,
      );
    }
  });

  it('keeps the language trees mirrored', () => {
    const [reference, ...others] = locales;
    const referenceTrails = [...(byLocale.get(reference)?.keys() ?? [])].sort();
    for (const locale of others) {
      const trails = [...(byLocale.get(locale)?.keys() ?? [])].sort();
      const missing = referenceTrails.filter((trail) => !trails.includes(trail));
      const extra = trails.filter((trail) => !referenceTrails.includes(trail));
      assert.deepEqual(
        { missing, extra },
        { missing: [], extra: [] },
        `${reference} and ${locale} must publish the same pages`,
      );
    }
  });

  it('declares every required frontmatter field', () => {
    for (const page of pages) {
      for (const field of requiredFields) {
        assert.ok(
          (page.fields[field] ?? '').length > 0,
          `${page.id}: frontmatter is missing "${field}"`,
        );
      }
      assert.ok(
        sections.includes(page.fields.section),
        `${page.id}: section "${page.fields.section}" is not one of ${sections.join(', ')}`,
      );
      assert.match(
        page.fields.alternate,
        /^\/(en|zh-cn)\//,
        `${page.id}: alternate must be a site path inside a locale`,
      );
    }
  });

  it('keeps frontmatter values parseable as YAML', () => {
    // A plain YAML scalar cannot contain a colon followed by a space: js-yaml
    // reads it as a nested mapping and fails the build, while this suite's line
    // parser would happily accept it. Catching it here turns a build failure into
    // a test failure that names the page.
    for (const page of pages) {
      const end = page.text.split('\n').indexOf('---', 1);
      const head = page.text.split('\n').slice(1, end === -1 ? undefined : end);
      for (const line of head) {
        const pair = /^([A-Za-z][\w-]*):\s(.*)$/.exec(line);
        if (pair === null) continue;
        const [, key, value] = pair;
        assert.ok(
          value.startsWith('"') || !value.includes(': '),
          `${page.id}: frontmatter "${key}" contains a colon and space; rephrase it or quote the value`,
        );
      }
    }
  });

  it('agrees between frontmatter section and directory layout', () => {
    for (const page of pages) {
      if (page.slug === 'index' && page.section === '') continue; // locale home page
      assert.equal(
        page.fields.section,
        page.section,
        `${page.id}: frontmatter section must match its directory`,
      );
    }
  });

  it('points each alternate at a page that exists and points back', () => {
    for (const page of pages) {
      const alternate = page.fields.alternate;
      const target = pages.find(
        (candidate) => `/${candidate.locale}/${candidate.trail}/`.replace(/\/index\/$/, '/') === alternate,
      );
      assert.ok(target, `${page.id}: alternate ${alternate} has no page`);
      assert.notEqual(
        target.locale,
        page.locale,
        `${page.id}: alternate ${alternate} must be in the other locale`,
      );
      assert.equal(
        target.fields.alternate,
        `/${page.locale}/${page.trail}/`.replace(/\/index\/$/, '/'),
        `${page.id}: alternate ${alternate} does not point back`,
      );
    }
  });

  it('publishes no empty placeholder page, and keeps drafts out of the build', () => {
    for (const page of pages) {
      if (isDraft(page)) {
        // A draft records an owed translation, so it must stay short: a long
        // draft means content is being written where a real page belongs.
        assert.ok(
          page.body.trim().length < MIN_BODY_LENGTH,
          `${page.id}: draft pages must stay short; write the page instead`,
        );
        continue;
      }
      assert.ok(
        page.body.trim().length >= MIN_BODY_LENGTH,
        `${page.id}: body is too short to be useful (${page.body.trim().length} characters)`,
      );
    }
  });

  it('gives every draft a counterpart page that is already written', () => {
    for (const page of pages.filter(isDraft)) {
      const alternate = page.fields.alternate;
      const target = pages.find(
        (candidate) =>
          `/${candidate.locale}/${candidate.trail}/`.replace(/\/index\/$/, '/') === alternate,
      );
      assert.ok(target, `${page.id}: draft alternate ${alternate} has no page`);
      assert.equal(
        isDraft(target),
        false,
        `${page.id}: both sides of a translation cannot be drafts`,
      );
    }
  });

  it('keeps section directories on disk', () => {
    for (const locale of locales) {
      for (const section of sections) {
        assert.ok(
          existsSync(join(contentRoot, locale, section)),
          `${locale}/${section} is declared as a section but has no directory`,
        );
      }
    }
  });
});
