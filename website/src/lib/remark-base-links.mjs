// Transform absolute Markdown links so they carry the configured `base` prefix.
//
// Astro rewrites links it renders itself, but Markdown bodies are emitted
// verbatim, so a link written as `/en/guide/` would leave the site and 404 on a
// GitHub Pages project path. Authors write site-root paths; this plugin adds the
// prefix during the build, which keeps local development and the published site
// consistent and avoids hard-coding the repository name in prose.
//
// The same transformation is applied by the link test, so a link that validates
// in the test is a link that renders correctly in the build.

/** Absolute paths already handled outside Markdown (assets live under `base` too). */
const REWRITABLE_PREFIXES = ['/en/', '/zh-cn/'];

/** True when the URL is an absolute site path this plugin is responsible for. */
export function isRewritable(url) {
  if (typeof url !== 'string' || !url.startsWith('/')) return false;
  if (url.startsWith('//')) return false; // protocol-relative
  return REWRITABLE_PREFIXES.some((prefix) => url === prefix.slice(0, -1) || url.startsWith(prefix));
}

/**
 * Build a remark plugin bound to one base prefix.
 *
 * @param {{ base: string }} options base path from the Astro config, e.g. `/Editor`
 * @returns {(tree: unknown) => void} remark transformer
 */
export function remarkBaseLinks({ base }) {
  const prefix = base.replace(/\/+$/, '');
  return (tree) => {
    visit(tree, (node) => {
      if (!node || node.type !== 'link' || !isRewritable(node.url)) return;
      // A trailing slash keeps the generated path stable for directory output.
      const url = node.url.endsWith('/') ? node.url : `${node.url}/`;
      node.url = `${prefix}${url}`;
    });
  };
}

/** Minimal unist walker: avoids a dependency for one node type. */
function visit(node, visitor) {
  visitor(node);
  const children = node && Array.isArray(node.children) ? node.children : [];
  for (const child of children) visit(child, visitor);
}
