/**
 * Navigation model for the documentation site.
 *
 * The two reader journeys are the top-level structure: editor usage for people
 * who installed the product, and the plugin contract for people who write
 * plugins against it. Sections are declared once here; sidebars are derived
 * from the content collection so adding a page never requires editing the
 * navigation by hand.
 */
import { getCollection, type CollectionEntry } from 'astro:content';

/**
 * Locales the site publishes, matching the i18n list in the Astro config.
 *
 * The Chinese locale is lower case on purpose: Astro lower-cases route
 * parameters, so a `zh-CN` directory would be emitted as `/zh-cn/` while every
 * link written as `/zh-CN/` would 404. Keeping the identifier lower case makes
 * the declared locale and the emitted route identical.
 */
export const LOCALES = ['en', 'zh-cn'] as const;

export type Locale = (typeof LOCALES)[number];

/** Locale labels shown in the language switcher. */
export const LOCALE_LABELS: Record<Locale, string> = {
  en: 'English',
  'zh-cn': '简体中文',
};

type SectionDefinition = {
  id: 'guide' | 'sdk';
  /** Section title per locale. */
  title: Record<Locale, string>;
  /** One-line purpose per locale, used on the home page. */
  summary: Record<Locale, string>;
};

export const SECTIONS: SectionDefinition[] = [
  {
    id: 'guide',
    title: { en: 'Editor guide', 'zh-cn': '编辑器使用' },
    summary: {
      en: 'Install, open a project, edit, navigate and configure the editor.',
      'zh-cn': '安装、打开项目、编辑、跳转与配置编辑器。',
    },
  },
  {
    id: 'sdk',
    title: { en: 'Plugin contract', 'zh-cn': '插件系统对接' },
    summary: {
      en: 'Capability negotiation, UI, services, processes and packaging for plugin authors.',
      'zh-cn': '面向插件作者的能力协商、界面、服务、进程与打包契约。',
    },
  },
];

/** Pages of one section in one locale, ordered by their declared sort key. */
export type SidebarItem = {
  href: string;
  title: string;
};

/**
 * Build the site-relative base for a locale.
 *
 * Astro applies `base` when it renders links, but plain data such as the
 * language switcher needs the same prefix computed here.
 */
export const localeBase = (base: string, locale: Locale): string => `${base}${locale}/`;

/** Path of a page inside a locale, always with a trailing slash. */
export const pagePath = (base: string, locale: Locale, trail: string): string =>
  `${base}${locale}/${trail.replace(/^\/+|\/+$/g, '')}/`;

/**
 * Sidebar entries for one section and locale.
 *
 * The section landing page (`.../<section>/index`) is intentionally excluded:
 * it is reachable from the home page and would otherwise duplicate the first
 * sidebar entry with identical text.
 */
export async function sidebarItems(
  base: string,
  locale: Locale,
  section: SectionDefinition['id'],
): Promise<SidebarItem[]> {
  const entries = await getCollection(
    'docs',
    (entry: CollectionEntry<'docs'>) =>
      entry.id.startsWith(`${locale}/${section}/`) && !entry.id.endsWith('/index'),
  );
  return entries
    .slice()
    .sort((left, right) => left.data.order - right.data.order)
    .map((entry) => ({
      href: pagePath(base, locale, entry.id.slice(locale.length + 1)),
      title: entry.data.title,
    }));
}

/** The counterpart page of the given entry, as declared in its frontmatter. */
export const alternateHref = (base: string, alternate: string): string => `${base}${alternate.replace(/^\//, '')}`;
