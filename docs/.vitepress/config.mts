import { defineConfig } from 'vitepress';
import { withMermaid } from 'vitepress-plugin-mermaid';

const APP = 'https://spool.itsanon.com';
const REPO = 'https://github.com/mrghxst/spool';

// The documentation site on GitHub Pages. The app itself is hosted at APP.
export default withMermaid(
  defineConfig({
    title: 'Spool',
    description: 'Usenet in a browser tab. Download, verify, repair and extract, with nothing to install.',
    base: '/spool/',
    cleanUrls: true,
    lastUpdated: false,
    // README files are for GitHub; package.json isn't a page.
    srcExclude: ['**/README.md'],
    head: [
      ['link', { rel: 'icon', href: '/spool/icon.svg', type: 'image/svg+xml' }],
      ['link', { rel: 'preload', href: '/spool/fonts/Geist-Variable.woff2', as: 'font', type: 'font/woff2', crossorigin: '' }],
      ['meta', { name: 'theme-color', content: '#000000' }],
    ],
    themeConfig: {
      logo: { light: '/icon.svg', dark: '/icon.svg', alt: '' },
      siteTitle: 'spool',
      nav: [
        { text: 'Guide', link: '/guide' },
        { text: 'Self-hosting', link: '/self-hosting' },
        { text: 'FAQ', link: '/faq' },
        { text: 'Open Spool', link: APP },
      ],
      sidebar: [
        {
          text: 'Using Spool',
          items: [
            { text: 'Guide', link: '/guide' },
            { text: 'FAQ', link: '/faq' },
            { text: 'Privacy', link: '/privacy' },
          ],
        },
        {
          text: 'Relay',
          items: [
            { text: 'Self-hosting', link: '/self-hosting' },
            { text: 'Protocol', link: '/protocol' },
          ],
        },
        {
          text: 'Internals',
          items: [
            { text: 'Architecture', link: '/architecture' },
            { text: 'Decisions', link: '/decisions' },
          ],
        },
      ],
      socialLinks: [{ icon: 'github', link: REPO, ariaLabel: 'Spool on GitHub' }],
      search: { provider: 'local' },
      outline: { level: [2, 3] },
      editLink: { pattern: `${REPO}/edit/main/docs/:path`, text: 'Edit this page on GitHub' },
      footer: {
        message: `<a href="${APP}">Open Spool</a> · <a href="${REPO}">Source on GitHub</a> · MIT license`,
      },
    },
    mermaid: { fontFamily: "'Geist', ui-sans-serif, system-ui, sans-serif" },
  }),
);
