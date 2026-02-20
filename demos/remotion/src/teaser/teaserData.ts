import timingPresets from './timing-presets.json';

export type Stack = 'rust' | 'node' | 'rails' | 'generic';
export type Format = 'landscape' | 'square' | 'vertical' | 'web-mobile';
export type Pace = 'full' | 'social';
export type Audience = 'marketing' | 'docs';

export type BranchBoxTeaserProps = {
  stack: Stack;
  format?: Format;
  pace?: Pace;
  audience?: Audience;
};

export type Scene = {
  command: string;
  mobileCommand?: string;
  verticalCommand?: string;
  title: string;
  mobileTitle?: string;
  painHook?: string;
  mobilePainHook?: string;
  proofBadges?: string[];
  mobileProofBadges?: string[];
  progressLabel: string;
  mobileProgressLabel?: string;
  output: string[];
  mobileOutput: string[];
  socialCommandTypingFrames?: number;
  socialOutputStartFrame?: number;
  socialOutputRevealEveryFrames?: number;
};

export type LayoutPreset = {
  rootPadding: string;
  topGap: number;
  heroGap: number;
  introTitleMaxWidth?: number;
  headingSize: number;
  subheadingSize: number;
  eyebrowSize: number;
  logoSize: number;
  cardTitleSize: number;
  cardBodySize: number;
  terminalHeight: number;
  terminalPadding: string;
  terminalGap: number;
  terminalHeaderSize: number;
  terminalCommandSize: number;
  terminalBodySize: number;
  terminalFooterSize: number;
  maxVisibleOutputLines: number;
  commandTypingFrames: number;
  outputStartFrame: number;
  outputRevealEveryFrames: number;
  cursorBlinkFrames: number;
};

export type StoryCopy = {
  heading: string;
  subheading: string;
  introTitle: string;
  introStory: string;
  outroEyebrow: string;
  outroTitle: string;
  outroLine1: string;
  outroLine2: string;
};

export const CAPTURED_AT = '2026-02-17';
export const SOCIAL_PAIN_HOOK_FRAMES = 42;
type TimingPreset = {introFrames: number; sceneFrames: number; outroFrames: number};
type TimingPresetBundle = {totalScenes: number} & Record<Pace, TimingPreset>;

const timingPresetBundle = timingPresets as TimingPresetBundle;

export const TOTAL_SCENES = timingPresetBundle.totalScenes;
export const TIMING_PRESETS: Record<Pace, TimingPreset> = {
  full: timingPresetBundle.full,
  social: timingPresetBundle.social,
};

export const presetDuration = (pace: Pace): number => {
  const preset = TIMING_PRESETS[pace];
  return preset.introFrames + preset.sceneFrames * TOTAL_SCENES + preset.outroFrames;
};

export const BRANCHBOX_TEASER_DURATION = presetDuration('full');
export const BRANCHBOX_TEASER_SOCIAL_DURATION = presetDuration('social');

export const BRAND = {
  bgPrimary: '#060915',
  bgSecondary: '#0a0a1a',
  bgTertiary: '#0d0d24',
  card: 'rgba(15, 15, 40, 0.75)',
  cardStrong: 'rgba(8, 8, 24, 0.88)',
  accent: '#8b5cf6',
  accentBright: '#a78bfa',
  accentGlow: 'rgba(139, 92, 246, 0.24)',
  cyan: '#22d3ee',
  success: '#22c55e',
  warning: '#f59e0b',
  textPrimary: '#f8fafc',
  textSecondary: '#cbd5e1',
  textDim: '#94a3b8',
};

type StackMeta = {project: string; adapter: string};

const STACK_META: Record<Stack, StackMeta> = {
  rust: {project: 'bbx-demo-rust', adapter: 'Generic · http://dev:3000'},
  node: {project: 'bbx-demo-node', adapter: 'Node.js · http://nodejs-app:3000'},
  rails: {project: 'bbx-demo-rails', adapter: 'Rails · http://rails-app:3000'},
  generic: {project: 'bbx-demo-generic', adapter: 'Generic · http://dev:3000'},
};

export const getStoryCopy = (audience: Audience, format: Format): StoryCopy => {
  if (audience === 'docs') {
    return {
      heading: 'BranchBox Workflow Walkthrough',
      subheading: 'Command-by-command clips for learning how feature worktrees behave in practice',
      introTitle: 'Documentation-focused capture',
      introStory: 'Story: setup, minimal mode, registry visibility, devcontainer sync (explained)',
      outroEyebrow: 'Docs CTA',
      outroTitle: 'Use chapter cuts while reading docs',
      outroLine1: 'Each section maps to one command and one outcome.',
      outroLine2: 'Guide: /docs/guides/demo-assets',
    };
  }

  if (format === 'square') {
    return {
      heading: 'BranchBox in 50 Seconds',
      subheading: 'Parallel feature environments with real CLI output and zero branch collisions',
      introTitle: 'Parallel features. Zero collisions.',
      introStory: 'Outcome-first capture: isolate, move fast, keep environments in sync.',
      outroEyebrow: 'Square Social CTA',
      outroTitle: 'Try BranchBox with your next feature',
      outroLine1: 'github.com/branchbox/branchbox',
      outroLine2: 'Share this reel with your engineering team.',
    };
  }

  if (format === 'vertical') {
    return {
      heading: 'BranchBox in 50 Seconds',
      subheading: 'Stop branch collisions by running each feature in its own isolated environment',
      introTitle: 'Fast proof, real workflow',
      introStory: 'Outcome-first capture: setup, minimal mode, visibility, safe sync.',
      outroEyebrow: 'Next Step',
      outroTitle: 'Launch collision-free branches today',
      outroLine1: 'branchbox.dev/docs/getting-started/quick-start',
      outroLine2: 'Flow: full setup -> minimal -> list -> sync',
    };
  }

  if (format === 'web-mobile') {
    return {
      heading: 'BranchBox in 50 Seconds',
      subheading: 'Parallel features with real CLI output and no branch collisions',
      introTitle: 'Fast proof, real workflow',
      introStory: 'Outcome-first capture: setup, minimal mode, visibility, safe sync.',
      outroEyebrow: 'Next Step',
      outroTitle: 'Launch collision-free branches today',
      outroLine1: 'branchbox.dev/docs/getting-started/quick-start',
      outroLine2: 'Flow: full setup -> minimal -> list -> sync',
    };
  }

  return {
    heading: 'BranchBox in 50 Seconds',
    subheading: 'Stop branch collisions by spinning up parallel feature environments from real CLI output',
    introTitle: 'From environment thrash to parallel flow',
    introStory: 'Real CLI capture: 4 commands, 2 active features, 0 branch collisions.',
    outroEyebrow: 'Website CTA',
    outroTitle: 'Run this in your repo today',
    outroLine1: 'Install: brew install branchbox/tap/branchbox',
    outroLine2: 'Quick start: branchbox.dev/docs/quick-start',
  };
};

export const getLayoutPreset = (format: Format): LayoutPreset => {
  switch (format) {
    case 'web-mobile':
      return {
        rootPadding: '46px 44px',
        topGap: 14,
        heroGap: 14,
        introTitleMaxWidth: 960,
        headingSize: 64,
        subheadingSize: 22,
        eyebrowSize: 18,
        logoSize: 24,
        cardTitleSize: 30,
        cardBodySize: 20,
        terminalHeight: 1010,
        terminalPadding: '24px 24px',
        terminalGap: 12,
        terminalHeaderSize: 22,
        terminalCommandSize: 34,
        terminalBodySize: 24,
        terminalFooterSize: 20,
        maxVisibleOutputLines: 6,
        commandTypingFrames: 44,
        outputStartFrame: 30,
        outputRevealEveryFrames: 11,
        cursorBlinkFrames: 10,
      };
    case 'square':
      return {
        rootPadding: '52px 58px',
        topGap: 16,
        heroGap: 18,
        introTitleMaxWidth: 860,
        headingSize: 72,
        subheadingSize: 28,
        eyebrowSize: 22,
        logoSize: 28,
        cardTitleSize: 34,
        cardBodySize: 24,
        terminalHeight: 690,
        terminalPadding: '24px 28px',
        terminalGap: 13,
        terminalHeaderSize: 22,
        terminalCommandSize: 34,
        terminalBodySize: 24,
        terminalFooterSize: 22,
        maxVisibleOutputLines: 7,
        commandTypingFrames: 46,
        outputStartFrame: 30,
        outputRevealEveryFrames: 12,
        cursorBlinkFrames: 10,
      };
    case 'vertical':
      return {
        rootPadding: '48px 42px',
        topGap: 14,
        heroGap: 14,
        introTitleMaxWidth: 920,
        headingSize: 66,
        subheadingSize: 24,
        eyebrowSize: 19,
        logoSize: 26,
        cardTitleSize: 30,
        cardBodySize: 21,
        terminalHeight: 1420,
        terminalPadding: '26px 24px',
        terminalGap: 14,
        terminalHeaderSize: 24,
        terminalCommandSize: 35,
        terminalBodySize: 26,
        terminalFooterSize: 22,
        maxVisibleOutputLines: 6,
        commandTypingFrames: 52,
        outputStartFrame: 34,
        outputRevealEveryFrames: 12,
        cursorBlinkFrames: 10,
      };
    default:
      return {
        rootPadding: '66px 80px',
        topGap: 18,
        heroGap: 22,
        introTitleMaxWidth: 1200,
        headingSize: 74,
        subheadingSize: 30,
        eyebrowSize: 21,
        logoSize: 27,
        cardTitleSize: 30,
        cardBodySize: 24,
        terminalHeight: 760,
        terminalPadding: '28px 34px',
        terminalGap: 16,
        terminalHeaderSize: 20,
        terminalCommandSize: 24,
        terminalBodySize: 20,
        terminalFooterSize: 20,
        maxVisibleOutputLines: 18,
        commandTypingFrames: 64,
        outputStartFrame: 60,
        outputRevealEveryFrames: 12,
        cursorBlinkFrames: 8,
      };
  }
};

const marketingScenes = (meta: StackMeta): Scene[] => {
  return [
    {
      title: 'Create a fully isolated feature environment',
      mobileTitle: 'Full setup',
      painHook: 'Port and env collisions break parallel feature work.',
      mobilePainHook: 'Port/env collisions kill focus.',
      proofBadges: ['2 active features isolated', '5/5 modules ready', '0 shared ports'],
      mobileProofBadges: ['5/5 modules ready', '0 shared ports'],
      progressLabel: 'Step 1/4 - Full setup',
      mobileProgressLabel: 'Step 1/4',
      command:
        'branchbox feature start "Add OAuth Integration" --skip-module compose --skip-module database',
      mobileCommand:
        'branchbox feature start "Add OAuth Integration"\n  --skip-module compose --skip-module database',
      verticalCommand:
        'branchbox feature start\n  "Add OAuth Integration"\n  --skip-module compose\n  --skip-module database',
      output: [
        '🚀 Feature workspace ready (full)',
        '  Feature: add-oauth',
        '✅ Worktree isolated: <tmp>/source/add-oauth',
        '✅ Branch ready: feature/add-oauth',
        `✅ Adapter detected: ${meta.adapter}`,
        '✅ Compose project isolated: source-add-oauth',
        '✅ .env scoped: App URL + compose vars injected',
        '✅ Modules: 5 ok / 0 skip',
        'Outcome: no collisions with your current branch.',
      ],
      mobileOutput: [
        '🚀 Feature workspace ready (full)',
        '  Feature: add-oauth',
        '✅ Worktree: <tmp>/source/add-oauth',
        '✅ Branch: feature/add-oauth',
        `✅ Adapter: ${meta.adapter}`,
        '✅ Compose: source-add-oauth',
        '✅ .env scoped',
        '✅ Modules: 5 ok / 0 skip',
      ],
      socialCommandTypingFrames: 14,
      socialOutputStartFrame: 4,
      socialOutputRevealEveryFrames: 5,
    },
    {
      title: 'Use minimal mode when speed matters',
      mobileTitle: 'Minimal mode',
      painHook: 'Full setup is overkill for quick fixes and spikes.',
      mobilePainHook: 'Need a faster path for quick fixes.',
      proofBadges: ['1 command minimal setup', '4 modules skipped', '281-char prompt captured'],
      mobileProofBadges: ['4 modules skipped', '281-char prompt captured'],
      progressLabel: 'Step 2/4 - Minimal mode',
      mobileProgressLabel: 'Step 2/4',
      command: 'branchbox feature new backlog-quick-fix --minimal --default-prompt',
      mobileCommand: 'branchbox feature new backlog-quick-fix\n  --minimal --default-prompt',
      verticalCommand:
        'branchbox feature new backlog-quick-fix\n  --minimal\n  --default-prompt',
      output: [
        '🚀 Feature workspace ready (minimal)',
        '  Feature: backlog-quick-fix',
        '✅ Branch: feature/backlog-quick-fix',
        `✅ Adapter: ${meta.adapter}`,
        '✅ Prompt seed stored: 281 chars',
        '⏭ Modules skipped: 4',
        '⏭ Recorded: devcontainer, compose, specs',
        'Next: run `branchbox devcontainer sync` when ready.',
      ],
      mobileOutput: [
        '🚀 Feature workspace ready (minimal)',
        '  Feature: backlog-quick-fix',
        '✅ Branch: feature/backlog-quick-fix',
        `✅ Adapter: ${meta.adapter}`,
        '✅ Prompt seed: 281 chars',
        '⏭ Modules skipped: 4',
        '⏭ Recorded: devcontainer, compose, specs',
        'Next: branchbox devcontainer sync',
      ],
      socialCommandTypingFrames: 14,
      socialOutputStartFrame: 4,
      socialOutputRevealEveryFrames: 5,
    },
    {
      title: 'Track active features in one command',
      mobileTitle: 'Feature visibility',
      painHook: 'Teams lose track of which feature environment is current.',
      mobilePainHook: 'Hard to track active feature envs.',
      proofBadges: ['2 active / 0 removed', 'Branch + mode visible', 'Registry in one command'],
      mobileProofBadges: ['2 active / 0 removed', 'One-command registry'],
      progressLabel: 'Step 3/4 - Feature visibility',
      mobileProgressLabel: 'Step 3/4',
      command: 'branchbox feature list',
      output: [
        '📚 Feature registry — 2 active · 0 removed',
        'backlog-quick-fix (minimal)',
        '  branch: feature/backlog-quick-fix',
        '  modules: 4 skip · devcontainer outdated',
        'add-oauth (full)',
        '  branch: feature/add-oauth',
        '  modules: 2 ok / 3 skip · devcontainer never',
        'Outcome: feature context is visible without context switching.',
      ],
      mobileOutput: [
        '📚 Feature registry — 2 active · 0 removed',
        'backlog-quick-fix (minimal)',
        '  branch: feature/backlog-quick-fix',
        '  modules: 4 skip',
        'add-oauth (full)',
        '  branch: feature/add-oauth',
        '  modules: 2 ok / 3 skip',
      ],
      socialCommandTypingFrames: 1,
      socialOutputStartFrame: 0,
      socialOutputRevealEveryFrames: 5,
    },
    {
      title: 'Propagate devcontainer updates safely',
      mobileTitle: 'Devcontainer sync',
      painHook: 'Devcontainer drift creates "works on my machine" chaos.',
      mobilePainHook: 'Devcontainer drift breaks parity.',
      proofBadges: ['Dry-run safety check', '2 worktrees synced', '0 destructive changes'],
      mobileProofBadges: ['2 worktrees synced', 'Dry-run safety'],
      progressLabel: 'Step 4/4 - Devcontainer sync',
      mobileProgressLabel: 'Step 4/4',
      command: 'branchbox devcontainer sync --dry-run',
      output: [
        '🔄 Syncing devcontainer configuration to 2 feature worktree(s)',
        'DRY RUN - no changes will be made',
        '  backlog-quick-fix ... would sync',
        '  add-oauth ... would sync',
        '✓ Successfully synced 2 feature worktree(s)',
        'Safe rollout: preview first, then apply.',
      ],
      mobileOutput: [
        '🔄 Syncing devcontainer configuration to 2 feature worktree(s)',
        'DRY RUN - no changes will be made',
        '  backlog-quick-fix ... would sync',
        '  add-oauth ... would sync',
        '✓ Successfully synced 2 feature worktree(s)',
        'Safe rollout: preview first.',
      ],
      socialCommandTypingFrames: 4,
      socialOutputStartFrame: 2,
      socialOutputRevealEveryFrames: 6,
    },
  ];
};

const docsScenes = (meta: StackMeta): Scene[] => {
  return [
    {
      title: 'Step 1: Full setup output explained',
      mobileTitle: 'Step 1: Full setup',
      progressLabel: 'Step 1/4 - Full setup (explained)',
      mobileProgressLabel: 'Step 1/4',
      command:
        'branchbox feature start "Add OAuth Integration" --skip-module compose --skip-module database',
      mobileCommand:
        'branchbox feature start "Add OAuth Integration"\n  --skip-module compose --skip-module database',
      verticalCommand:
        'branchbox feature start\n  "Add OAuth Integration"\n  --skip-module compose\n  --skip-module database',
      output: [
        '🚀 Feature workspace ready (full)',
        '  Feature: add-oauth',
        'Worktree: <tmp>/source/add-oauth',
        'Branch: feature/add-oauth',
        `Adapter: ${meta.adapter}`,
        'Compose project: source-add-oauth',
        '.env: App URL + compose vars injected',
        'Modules: 5 ok / 0 skip',
        'Why it matters: full parity without touching main.',
      ],
      mobileOutput: [
        '🚀 Feature workspace ready (full)',
        '  Feature: add-oauth',
        'Worktree: <tmp>/source/add-oauth',
        'Branch: feature/add-oauth',
        `Adapter: ${meta.adapter}`,
        'Compose: source-add-oauth',
        '.env scoped for this feature',
        'Modules: 5 ok / 0 skip',
      ],
      socialCommandTypingFrames: 14,
      socialOutputStartFrame: 4,
      socialOutputRevealEveryFrames: 5,
    },
    {
      title: 'Step 2: Minimal mode output explained',
      mobileTitle: 'Step 2: Minimal mode',
      progressLabel: 'Step 2/4 - Minimal mode (explained)',
      mobileProgressLabel: 'Step 2/4',
      command: 'branchbox feature new backlog-quick-fix --minimal --default-prompt',
      mobileCommand: 'branchbox feature new backlog-quick-fix\n  --minimal --default-prompt',
      verticalCommand:
        'branchbox feature new backlog-quick-fix\n  --minimal\n  --default-prompt',
      output: [
        '🚀 Feature workspace ready (minimal)',
        '  Feature: backlog-quick-fix',
        'Branch: feature/backlog-quick-fix',
        `Adapter: ${meta.adapter}`,
        'Prompt seed stored: 281 chars',
        'Modules skipped: 4',
        'Recorded skips: devcontainer, compose, specs',
        'Use this mode when you want a fast, lightweight start.',
      ],
      mobileOutput: [
        '🚀 Feature workspace ready (minimal)',
        '  Feature: backlog-quick-fix',
        'Branch: feature/backlog-quick-fix',
        `Adapter: ${meta.adapter}`,
        'Prompt seed: 281 chars',
        'Modules skipped: 4',
        'Recorded: devcontainer, compose, specs',
        'Fast path for small changes.',
      ],
      socialCommandTypingFrames: 14,
      socialOutputStartFrame: 4,
      socialOutputRevealEveryFrames: 5,
    },
    {
      title: 'Step 3: Feature registry output explained',
      mobileTitle: 'Step 3: Feature list',
      progressLabel: 'Step 3/4 - Feature visibility (explained)',
      mobileProgressLabel: 'Step 3/4',
      command: 'branchbox feature list',
      output: [
        '📚 Feature registry — 2 active · 0 removed',
        'backlog-quick-fix',
        '  mode: minimal',
        '  branch: feature/backlog-quick-fix',
        '  modules: 4 skip',
        'add-oauth',
        '  mode: full',
        '  branch: feature/add-oauth',
        '  modules: 2 ok / 3 skip',
        'Use this to decide what to sync or teardown.',
      ],
      mobileOutput: [
        '📚 Feature registry — 2 active · 0 removed',
        'backlog-quick-fix',
        '  mode: minimal',
        '  branch: feature/backlog-quick-fix',
        '  modules: 4 skip',
        'add-oauth',
        '  mode: full',
        '  branch: feature/add-oauth',
      ],
      socialCommandTypingFrames: 1,
      socialOutputStartFrame: 0,
      socialOutputRevealEveryFrames: 5,
    },
    {
      title: 'Step 4: Devcontainer sync output explained',
      mobileTitle: 'Step 4: Devcontainer sync',
      progressLabel: 'Step 4/4 - Devcontainer sync (explained)',
      mobileProgressLabel: 'Step 4/4',
      command: 'branchbox devcontainer sync --dry-run',
      output: [
        '🔄 Syncing devcontainer configuration to 2 feature worktree(s)',
        'DRY RUN - no changes will be made',
        '  backlog-quick-fix ... would sync',
        '  add-oauth ... would sync',
        '✓ Successfully synced 2 feature worktree(s)',
        'Re-run without --dry-run to apply updates.',
        'Tip: use this after editing .devcontainer files.',
      ],
      mobileOutput: [
        '🔄 Syncing devcontainer configuration to 2 feature worktree(s)',
        'DRY RUN - no changes will be made',
        '  backlog-quick-fix ... would sync',
        '  add-oauth ... would sync',
        '✓ Successfully synced 2 feature worktree(s)',
        'Re-run without --dry-run to apply.',
      ],
      socialCommandTypingFrames: 4,
      socialOutputStartFrame: 2,
      socialOutputRevealEveryFrames: 6,
    },
  ];
};

export const buildScenes = (stack: Stack, audience: Audience = 'marketing'): Scene[] => {
  const meta = STACK_META[stack];
  return audience === 'docs' ? docsScenes(meta) : marketingScenes(meta);
};
