// The docs site: Starlight, served at strata-lang.dev/docs. The pages come
// from ../docs/*.md (sync.mjs); the build goes into ../site/docs/, next to the
// landing page and the tour.
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import { readFileSync } from 'node:fs';

const json = f => JSON.parse(readFileSync(new URL(f, import.meta.url), 'utf8'));

export default defineConfig({
  site: 'https://strata-lang.dev',
  base: '/docs',
  outDir: '../site/docs',
  trailingSlash: 'ignore',
  integrations: [
    starlight({
      title: 'Strata',
      description: 'An experimental programming language for working with agents.',
      logo: { src: './src/logo.svg', alt: '' },
      favicon: '/favicon.svg',
      social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/anonx3247/strata' }],
      customCss: ['./src/strata.css'],
      components: { SocialIcons: './src/SocialIcons.astro' },
      head: [
        { tag: 'link', attrs: { rel: 'preconnect', href: 'https://fonts.gstatic.com', crossorigin: '' } },
        { tag: 'link', attrs: { rel: 'stylesheet', href: 'https://fonts.googleapis.com/css2?family=Fira+Code:wght@400;500;600&family=Inter:wght@400;500;600&family=Space+Grotesk:wght@500;600&display=swap' } },
      ],
      expressiveCode: {
        themes: [json('./src/strata-dark.json'), 'github-light'],
        shiki: { langs: [json('./src/strata.tmLanguage.json')] },
        styleOverrides: { borderRadius: '10px', codeFontFamily: '"Fira Code", ui-monospace, monospace', codeFontSize: '0.86rem' },
      },
      sidebar: [
        { label: 'Guide', items: [{ label: 'Overview', link: '/' }, 'language', 'verification', 'tools'] },
        { label: 'Annex', items: ['learn-strata', 'language-reference', 'verification-reference', 'tools-reference'] },
        { label: 'Internals', items: ['internals', 'binary-format'] },
      ],
      lastUpdated: false,
    }),
  ],
});
