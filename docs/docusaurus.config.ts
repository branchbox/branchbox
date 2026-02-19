import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

// This runs in Node.js - Don't use client-side code here (browser APIs, JSX...)

const config: Config = {
  title: 'BranchBox',
  tagline: 'Parallel development for humans and AI agents',
  favicon: 'img/logo.svg',

  // Future flags, see https://docusaurus.io/docs/api/docusaurus-config#future
  future: {
    v4: true, // Improve compatibility with the upcoming Docusaurus v4
  },

  // Set the production url of your site here
  url: 'https://branchbox.dev',
  // Set the /<baseUrl>/ pathname under which your site is served
  // Docs live at /docs/ while landing page is at root
  baseUrl: '/docs/',

  // GitHub pages deployment config.
  // If you aren't using GitHub pages, you don't need these.
  organizationName: 'branchbox', // Usually your GitHub org/user name.
  projectName: 'branchbox', // Usually your repo name.

  onBrokenLinks: process.env.CI ? 'throw' : 'warn',
  onBrokenMarkdownLinks: process.env.CI ? 'throw' : 'warn',

  // Even if you don't use internationalization, you can use this field to set
  // useful metadata like html lang. For example, if your site is Chinese, you
  // may want to replace "en" with "zh-Hans".
  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },

  presets: [
    [
      'classic',
      {
        docs: {
          sidebarPath: './sidebars.ts',
          editUrl: 'https://github.com/branchbox/branchbox/tree/main/docs',
          routeBasePath: '/', // Serve docs at the root
        },
        blog: false, // Disable blog for now
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    // Social card for sharing
    image: 'media/demos/branchbox-teaser-rust-social-card.jpg',

    // Force dark mode to match the landing page
    colorMode: {
      defaultMode: 'dark',
      disableSwitch: false,
      respectPrefersColorScheme: false,
    },

    // Announcement bar (optional)
    announcementBar: {
      id: 'support_us',
      content:
        '⭐ If you like BranchBox, give it a star on <a target="_blank" rel="noopener noreferrer" href="https://github.com/branchbox/branchbox">GitHub</a>!',
      backgroundColor: 'rgba(139, 92, 246, 0.15)',
      textColor: '#f8fafc',
      isCloseable: true,
    },

    navbar: {
      title: 'BranchBox',
      logo: {
        alt: 'BranchBox Logo',
        src: 'img/logo.svg',
        href: 'https://branchbox.dev/',  // Absolute URL to bypass baseUrl
        target: '_self',
      },
      items: [
        {
          href: 'https://branchbox.dev/',  // Absolute URL to bypass baseUrl
          label: 'Home',
          position: 'left',
          target: '_self',
        },
        {
          type: 'docSidebar',
          sidebarId: 'docsSidebar',
          position: 'left',
          label: 'Docs',
        },
        {
          to: '/guides/parallel-features',
          label: 'Guides',
          position: 'left',
        },
        {
          to: '/reference/cli',
          label: 'CLI',
          position: 'left',
        },
        {
          href: 'https://github.com/branchbox/branchbox/releases',
          label: 'Releases',
          position: 'right',
        },
        {
          href: 'https://github.com/branchbox/branchbox',
          label: 'GitHub',
          position: 'right',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: 'Learn',
          items: [
            {
              label: 'Quick Start',
              to: '/getting-started/quick-start',
            },
            {
              label: 'How It Works',
              to: '/how-it-works',
            },
            {
              label: 'CLI Reference',
              to: '/reference/cli',
            },
          ],
        },
        {
          title: 'Guides',
          items: [
            {
              label: 'Parallel Features',
              to: '/guides/parallel-features',
            },
            {
              label: 'AI Agents',
              to: '/guides/ai-agents',
            },
            {
              label: 'Minimal Mode',
              to: '/guides/minimal-mode',
            },
          ],
        },
        {
          title: 'Community',
          items: [
            {
              label: 'Website',
              href: 'https://branchbox.dev/',
            },
            {
              label: 'GitHub',
              href: 'https://github.com/branchbox/branchbox',
            },
            {
              label: 'Issues',
              href: 'https://github.com/branchbox/branchbox/issues',
            },
            {
              label: 'Discussions',
              href: 'https://github.com/branchbox/branchbox/discussions',
            },
          ],
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} BranchBox Contributors. MIT License.`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.vsDark,
      additionalLanguages: ['bash', 'json', 'toml', 'rust'],
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
