// Turns the repository's docs (../docs/*.md, the files GitHub shows) into the
// site's pages in src/content/docs/: each gets its `# Title` as frontmatter,
// Strata code fences are marked `strata` (they say `rust` so GitHub colours
// them), the tools' output `strata-output`, links between the docs point at their pages, and links to other files
// of the repository go to GitHub. The standard library's reference,
// ../docs/std/ (written by `strata --file std doc -o docs/std`), becomes the
// pages under std/, its README.md their index. src/content/docs/ is
// generated: edit ../docs/.
import { readFileSync, writeFileSync, rmSync, mkdirSync, cpSync, readdirSync } from 'node:fs';

const DOCS = new URL('../docs/', import.meta.url), OUT = new URL('./src/content/docs/', import.meta.url);
const REPO = 'https://github.com/anonx3247/strata/blob/main/';
// the pages, in the sidebar's order (astro.config.mjs groups them)
const PAGES = ['language', 'verification', 'tools', 'learn-strata', 'language-reference', 'verification-reference', 'tools-reference', 'internals', 'binary-format'];
// std's reference: a page per module (`std/io`), the index `std/README`
const STD = readdirSync(new URL('std/', DOCS)).filter(f => f.endsWith('.md')).map(f => 'std/' + f.replace(/\.md$/, ''));
// a page's path under docs/ (without `.md`) → its slug on the site
const slug = name => name === 'std/README' ? 'std' : name;

rmSync(OUT, { recursive: true, force: true });
mkdirSync(new URL('std/', OUT), { recursive: true });
for (const f of readdirSync(new URL('./pages/', import.meta.url))) cpSync(new URL(`./pages/${f}`, import.meta.url), new URL(f, OUT));

for (const page of [...PAGES, ...STD]) {
  let md = readFileSync(new URL(`${page}.md`, DOCS), 'utf8');
  const title = md.match(/^# (.+)$/m)[1];
  md = md.replace(/^# .+\n+/, '')
    .replace(/^Strata code is highlighted as Rust, the closest match\.\s*/m, '')
    .replace(/^```rust$/gm, '```strata')
    // what the tools print: coloured as the tour colours it
    .replace(/^```text\n(?=(\$ |\s*[✓✗~○≈] ))/gm, '```strata-output\n');
  // links, outside code (relative to the page's own folder)
  const here = new URL(page, 'https://repo/docs/');
  md = md.split(/(^```[\s\S]*?^```$)/m).map((part, i) => i % 2 ? part : part.replace(/\]\(([^)\s]+)\)/g, (all, href) => {
    if (/^(https?:|#)/.test(href)) return all;
    const [file, frag] = href.split('#');
    if (!/\.\w+$|\/$|^\.\./.test(file)) return all; // not a path, e.g. `[T](x: T)` in prose
    const path = new URL(file, here).pathname;
    const name = path.replace(/^\/docs\//, '').replace(/\.md$/, '');
    if (path.startsWith('/docs/') && (PAGES.includes(name) || STD.includes(name))) return `](/docs/${slug(name)}/${frag ? '#' + frag : ''})`;
    return `](${REPO}${path.slice(1)}${frag ? '#' + frag : ''})`;
  })).join('');
  const std = page.startsWith('std/');
  const order = std ? (page === 'std/README' ? 0 : STD.indexOf(page) + 1) : PAGES.indexOf(page);
  const label = page === 'std/README' ? '\n  label: Overview' : '';
  const front = `---\ntitle: ${JSON.stringify(std && page === 'std/README' ? 'The standard library' : title)}\nsidebar:\n  order: ${order}${label}\n---\n\n`;
  writeFileSync(new URL(`${page === 'std/README' ? 'std/index' : page}.md`, OUT), front + md);
}
console.log(`sync: ${PAGES.length} pages from docs/, ${STD.length} from docs/std/`);
