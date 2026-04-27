import { defineConfig } from 'vitepress';

export default defineConfig({
  title: 'Sonium',
  description: 'Open-source multiroom audio for local networks',
  base: '/sonium/',
  outDir: '../dist',
  cleanUrls: true,
  lastUpdated: true,
  appearance: 'dark',
  head: [
    ['meta', { name: 'theme-color', content: '#0f172a' }],
    ['link', { rel: 'icon', href: '/sonium/favicon.svg' }],
  ],
  themeConfig: {
    logo: '/favicon.svg',
    siteTitle: 'Sonium',
    nav: [
      { text: 'Guide', link: '/getting-started/quick-start' },
      { text: 'Reference', link: '/reference/config' },
      { text: 'Architecture', link: '/architecture/overview' },
      { text: 'GitHub', link: 'https://github.com/jdavidoa91/sonium' },
    ],
    sidebar: [
      {
        text: 'Start Here',
        items: [
          { text: 'Introduction', link: '/introduction' },
          { text: 'Quick Start', link: '/getting-started/quick-start' },
          { text: 'Installation', link: '/getting-started/installation' },
          { text: 'Configuration', link: '/getting-started/configuration' },
        ],
      },
      {
        text: 'Architecture',
        items: [
          { text: 'Overview', link: '/architecture/overview' },
          { text: 'Workspace Layout', link: '/architecture/workspace' },
          { text: 'Clock Sync', link: '/architecture/sync' },
          { text: 'Audio Pipeline', link: '/architecture/pipeline' },
          { text: 'Web UI', link: '/architecture/web-ui' },
        ],
      },
      {
        text: 'Reference',
        items: [
          { text: 'Config Reference', link: '/reference/config' },
          { text: 'REST API', link: '/reference/api' },
          { text: 'Binary Protocol', link: '/reference/protocol' },
        ],
      },
      {
        text: 'Contributing',
        items: [
          { text: 'Development Setup', link: '/contributing/dev-setup' },
          { text: 'Roadmap', link: '/contributing/roadmap' },
        ],
      },
    ],
    socialLinks: [
      { icon: 'github', link: 'https://github.com/jdavidoa91/sonium' },
    ],
    search: {
      provider: 'local',
    },
    footer: {
      message: 'Local-first multiroom audio. No cloud account required.',
      copyright: 'Released under GPL-3.0',
    },
  },
});
