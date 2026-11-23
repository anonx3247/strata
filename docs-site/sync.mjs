// Turns the repository's docs (../docs/*.md, the files GitHub shows) into the
// site's pages in src/content/docs/: each gets its `# Title` as frontmatter,
// Strata code fences are marked `strata` (they say `rust` so GitHub colours
// them), links between the docs point at their pages, and links to other files
// of the repository go to GitHub. src/content/docs/ is generated: edit ../docs/.
import { readFileSync, writeFileSync, rmSync, mkdirSync, cpSync, readdirSync } from 'node:fs';

const DOCS = new URL('../docs/', import.meta.url), OUT = new URL('./src/content/docs/', import.meta.url);
const REPO = 'https://github.com/anonx3247/strata/blob/main/';
// the pages, in the sidebar's order (astro.config.mjs groups them)
const PAGES = ['language', 'verification', 'tools', 'learn-strata', 'language-reference', 'verification-reference', 'tools-reference', 'internals', 'binary-format'];

rmSync(OUT, { recursive: true, force: true });
mkdirSync(OUT, { recursive: true });
cpSync(new URL('images/', DOCS), new URL('images/', OUT), { recursive: true });
for (const f of readdirSync(new URL('./pages/', import.meta.url))) cpSync(new URL(`./pages/${f}`, import.meta.url), new URL(f, OUT));

for (const page of PAGES) {
  let md = readFileSync(new URL(`${page}.md`, DOCS), 'utf8');
  const title = md.match(/^# (.+)$/m)[1];
  md = md.replace(/^# .+\n+/, '')
    .replace(/^Strata code is highlighted as Rust, the closest match\.\s*/m, '')
    .replace(/^```rust$/gm, '```strata');
  // links, outside code
  md = md.split(/(^```[\s\S]*?^```$)/m).map((part, i) => i % 2 ? part : part.replace(/\]\(([^)\s]+)\)/g, (all, href) => {
    if (/^(https?:|#|images\/)/.test(href)) return all;
    const [file, frag] = href.split('#');
    const name = file.replace(/^\.\/|\.md$/g, '');
    if (PAGES.includes(name)) return `](/docs/${name}/${frag ? '#' + frag : ''})`;
    if (!/\.\w+$|\/$|^\.\./.test(file)) return all; // not a path, e.g. `[T](x: T)` in prose
    return `](${REPO}${new URL(file, 'https://repo/docs/').pathname.slice(1)}${frag ? '#' + frag : ''})`;
  })).join('');
  const front = `---\ntitle: ${JSON.stringify(title)}\nsidebar:\n  order: ${PAGES.indexOf(page)}\n---\n\n`;
  writeFileSync(new URL(`${page}.md`, OUT), front + md);
}
console.log(`sync: ${PAGES.length} pages from docs/`);
