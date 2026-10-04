// Content collection for the bilingual documentation tree.
//
// Both language trees live in the same collection so a single schema validates
// them and the page-set test can compare the trees without parsing Markdown.
// Entry ids keep the locale prefix, for example `en/guide/index`, which is what
// makes the mirrored tree check a set comparison rather than a heuristic.
import { defineCollection, z } from 'astro:content';
import { glob } from 'astro/loaders';

/** Sections shared by both language trees. */
export const sections = ['guide', 'sdk'] as const;

/** Locales the site publishes; must match the i18n list in astro.config.mjs. */
export const locales = ['en', 'zh-cn'] as const;

export type Section = (typeof sections)[number];

export type Locale = (typeof locales)[number];

/**
 * Split a collection entry id such as `zh-cn/sdk/ui` into its parts.
 *
 * The locale is always the first segment and the final segment is the page
 * slug, so `guide/index` becomes slug `index` inside the `guide` section.
 */
export function splitEntryId(id: string): { locale: string; section: string; slug: string } {
  const parts = id.split('/');
  const locale = parts[0] ?? '';
  const slug = parts[parts.length - 1] ?? '';
  const section = parts.length > 2 ? parts.slice(1, -1).join('/') : '';
  return { locale, section, slug };
}

/**
 * Path of the mirrored page inside the other language tree.
 *
 * Readers switch languages far more often than they switch topics, so every
 * page declares where its counterpart lives; the page-set test asserts the
 * declaration is reciprocal so a half-finished translation cannot ship.
 */
export const alternateUrl = (locale: string, id: string): string => {
  const other = locale === 'en' ? 'zh-cn' : 'en';
  const trail = id.split('/').slice(1).join('/');
  return `/${other}/${trail}/`;
};

export const docs = defineCollection({
  loader: glob({ pattern: '**/*.md', base: './src/content/docs' }),
  schema: z.object({
    /** Page title shown in the sidebar, the browser tab and the page heading. */
    title: z.string().min(1),
    /** One-line summary used by the section index and as the meta description. */
    description: z.string().min(1),
    /** Which of the two reader journeys the page belongs to. */
    section: z.enum(sections),
    /** Sort key inside the section; lower numbers come first. */
    order: z.number().int(),
    /** Counterpart page in the other language tree. */
    alternate: z.string().startsWith('/'),
  }),
});

export const collections = { docs };
