/** @type {import('@docusaurus/plugin-content-docs').SidebarsConfig} */
const sidebars = {
  docs: [
    'intro',
    {
      type: 'category',
      label: 'Getting Started',
      collapsible: false,
      items: [
        'getting-started/installation',
        'getting-started/development-environment',
      ],
    },
    {
      type: 'category',
      label: 'Architecture',
      items: ['architecture/overview'],
    },
    {
      type: 'category',
      label: 'Guides',
      items: [
        'guides/homebrew-setup',
        'guides/init-parent-structure',
      ],
    },
    {
      type: 'category',
      label: 'Protocol',
      items: ['protocol/overview'],
    },
    {
      type: 'category',
      label: 'Reference',
      items: ['reference/cli', 'reference/specs-workflow'],
    },
    {
      type: 'category',
      label: 'Roadmap',
      items: ['roadmap/implementation-status'],
    },
  ],
};

module.exports = sidebars;
