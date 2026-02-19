import timingPresets from './timing-presets.json';

export type Stack = 'rust' | 'node' | 'rails' | 'generic';
export type Format = 'landscape' | 'square' | 'vertical';
export type Pace = 'full' | 'social';

export type BranchBoxTeaserProps = {
  stack: Stack;
  format?: Format;
  pace?: Pace;
};

export type Scene = {
  command: string;
  mobileCommand?: string;
  verticalCommand?: string;
  title: string;
  mobileTitle?: string;
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

export const CAPTURED_AT = '2026-02-17';
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

const STACK_META: Record<Stack, {project: string; adapter: string}> = {
  rust: {project: 'bbx-demo-rust', adapter: 'Generic · http://dev:3000'},
  node: {project: 'bbx-demo-node', adapter: 'Node.js · http://nodejs-app:3000'},
  rails: {project: 'bbx-demo-rails', adapter: 'Rails · http://rails-app:3000'},
  generic: {project: 'bbx-demo-generic', adapter: 'Generic · http://dev:3000'},
};

export const getLayoutPreset = (format: Format): LayoutPreset => {
  switch (format) {
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
        terminalCommandSize: 31,
        terminalBodySize: 22,
        terminalFooterSize: 22,
        maxVisibleOutputLines: 8,
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
        terminalHeaderSize: 23,
        terminalCommandSize: 27,
        terminalBodySize: 22,
        terminalFooterSize: 20,
        maxVisibleOutputLines: 8,
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

export const buildScenes = (stack: Stack): Scene[] => {
  const meta = STACK_META[stack];

  return [
    {
      title: 'Launch a full feature workspace',
      mobileTitle: 'Full setup',
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
        '',
        '+-----------------+----------------+--------------------------------------------+',
        '| Step            | Result         | Details                                    |',
        '+-----------------+----------------+--------------------------------------------+',
        '| Worktree        | ✅ ready        | <tmp>/source/add-oauth                     |',
        '| Branch          | ✅ ready        | feature/add-oauth                          |',
        `| Adapter         | ✅ detected     | ${meta.adapter}`,
        '| Compose project | ✅ isolated     | source-add-oauth                           |',
        '| .env            | ✅ copied       | App URL + compose vars injected            |',
        '| Modules         | ✅ ready        | 5 ok / 0 skip                              |',
        '+-----------------+----------------+--------------------------------------------+',
      ],
      mobileOutput: [
        '🚀 Feature workspace ready (full)',
        '  Feature: add-oauth',
        '',
        '✅ Worktree: <tmp>/source/add-oauth',
        '✅ Branch: feature/add-oauth',
        `✅ Adapter: ${meta.adapter}`,
        '✅ Compose project: source-add-oauth',
        '✅ .env copied: App URL + compose vars injected',
        '✅ Modules: 5 ok / 0 skip',
      ],
      socialCommandTypingFrames: 14,
      socialOutputStartFrame: 4,
      socialOutputRevealEveryFrames: 5,
    },
    {
      title: 'Use minimal mode for instant setup',
      mobileTitle: 'Minimal mode',
      progressLabel: 'Step 2/4 - Minimal mode',
      mobileProgressLabel: 'Step 2/4',
      command: 'branchbox feature new backlog-quick-fix --minimal --default-prompt',
      mobileCommand: 'branchbox feature new backlog-quick-fix\n  --minimal --default-prompt',
      verticalCommand:
        'branchbox feature new backlog-quick-fix\n  --minimal\n  --default-prompt',
      output: [
        '🚀 Feature workspace ready (minimal)',
        '  Feature: backlog-quick-fix',
        '',
        '+-----------------+----------------+--------------------------------------------+',
        '| Step            | Result         | Details                                    |',
        '+-----------------+----------------+--------------------------------------------+',
        '| Branch          | ✅ ready        | feature/backlog-quick-fix                  |',
        `| Adapter         | ✅ detected     | ${meta.adapter}`,
        '| Prompt seed     | ✅ stored       | 281 chars (bridge disabled)                |',
        '| Modules         | ⏭ skipped      | 4 skip                                     |',
        '| Skipped modules | ⏭ recorded     | devcontainer, compose, specs               |',
        '+-----------------+----------------+--------------------------------------------+',
        'Next: run `branchbox devcontainer sync` or targeted module commands when ready.',
      ],
      mobileOutput: [
        '🚀 Feature workspace ready (minimal)',
        '  Feature: backlog-quick-fix',
        '',
        '✅ Branch: feature/backlog-quick-fix',
        `✅ Adapter: ${meta.adapter}`,
        '✅ Prompt seed stored: 281 chars',
        '⏭ Modules skipped: 4',
        '⏭ Recorded: devcontainer, compose, specs',
        'Next: run `branchbox devcontainer sync` when ready.',
      ],
      socialCommandTypingFrames: 14,
      socialOutputStartFrame: 4,
      socialOutputRevealEveryFrames: 5,
    },
    {
      title: 'Track every feature branch at a glance',
      mobileTitle: 'Feature visibility',
      progressLabel: 'Step 3/4 - Feature visibility',
      mobileProgressLabel: 'Step 3/4',
      command: 'branchbox feature list',
      output: [
        '📚 Feature registry — 2 active · 0 removed (showing 2/2)',
        'Feature            Status  Mode     Prompt            Modules        Branch                     URL  Tunnel    Devcontainer  Agent     PR   Color    Updated',
        '-----------------  ------  -------  ----------------  -------------  -------------------------  ---  --------  ------------  --------  ---  -------  ----------------',
        'backlog-quick-fix  active  minimal  seed (281 chars)  4 skip         feature/backlog-quick-fix  —    disabled  outdated      disabled  —    #e67e22  2026-02-17 15:44',
        'add-oauth          active  full     —                 2 ok / 3 skip  feature/add-oauth          —    disabled  never         disabled  —    #8e44ad  2026-02-17 15:44',
      ],
      mobileOutput: [
        '📚 Feature registry — 2 active · 0 removed',
        '',
        'backlog-quick-fix',
        '  mode: minimal',
        '  branch: feature/backlog-quick-fix',
        '  modules: 4 skip',
        '',
        'add-oauth',
        '  mode: full',
        '  branch: feature/add-oauth',
        '  modules: 2 ok / 3 skip',
      ],
      socialCommandTypingFrames: 1,
      socialOutputStartFrame: 0,
      socialOutputRevealEveryFrames: 5,
    },
    {
      title: 'Sync devcontainer updates safely',
      mobileTitle: 'Devcontainer sync',
      progressLabel: 'Step 4/4 - Devcontainer sync',
      mobileProgressLabel: 'Step 4/4',
      command: 'branchbox devcontainer sync --dry-run',
      output: [
        '🔄 Syncing devcontainer configuration to 2 feature worktree(s)',
        '',
        'DRY RUN - no changes will be made',
        '',
        '  backlog-quick-fix ... would sync',
        '  add-oauth ... would sync',
        '',
        '✓ Successfully synced 2 feature worktree(s)',
      ],
      mobileOutput: [
        '🔄 Syncing devcontainer configuration to 2 feature worktree(s)',
        '',
        'DRY RUN - no changes will be made',
        '  backlog-quick-fix ... would sync',
        '  add-oauth ... would sync',
        '',
        '✓ Successfully synced 2 feature worktree(s)',
      ],
      socialCommandTypingFrames: 4,
      socialOutputStartFrame: 2,
      socialOutputRevealEveryFrames: 6,
    },
  ];
};
