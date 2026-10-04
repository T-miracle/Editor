// Astro configuration for the reader-facing documentation site.
//
// The site is published as a GitHub Pages project site, so every URL carries the
// repository name as a path prefix. `base` and `site` must be changed together
// whenever the repository is renamed or moved to a custom domain; a mismatch
// silently breaks every internal link because Astro emits them relative to `base`.
import { defineConfig } from 'astro/config';
import { unified } from '@astrojs/markdown-remark';
import { remarkBaseLinks } from './src/lib/remark-base-links.mjs';

/** Repository name used as the GitHub Pages project path. */
const repositoryName = 'Editor';
const base = `/${repositoryName}/`;

export default defineConfig({
  site: `https://t-miracle.github.io${base}`,
  base,
  trailingSlash: 'ignore',
  output: 'static',
  outDir: './dist',
  i18n: {
    defaultLocale: 'en',
    locales: ['en', 'zh-cn'],
    routing: {
      // The default locale keeps its prefix so that both language trees stay
      // symmetric; a symmetric tree is what makes the page-set test meaningful.
      prefixDefaultLocale: true,
    },
  },
  markdown: {
    // Astro 7 defaults to the Sätteri processor; the remark pipeline is opt-in
    // and is what the base-prefix link transform needs.
    processor: unified({
      remarkPlugins: [[remarkBaseLinks, { base }]],
    }),
  },
});
