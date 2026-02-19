import type React from 'react';
import {AbsoluteFill, interpolate, spring, useCurrentFrame, useVideoConfig} from 'remotion';

type Stack = 'rust' | 'node' | 'rails' | 'generic';
type Format = 'landscape' | 'square' | 'vertical';
type Pace = 'full' | 'social';

export type BranchBoxTeaserProps = {
  stack: Stack;
  format?: Format;
  pace?: Pace;
};

type Scene = {
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

type LayoutPreset = {
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

const CAPTURED_AT = '2026-02-17';
const TOTAL_SCENES = 4;

const TIMING_PRESETS: Record<Pace, {introFrames: number; sceneFrames: number; outroFrames: number}> = {
  full: {introFrames: 120, sceneFrames: 330, outroFrames: 90},
  social: {introFrames: 60, sceneFrames: 210, outroFrames: 45},
};

const presetDuration = (pace: Pace): number => {
  const preset = TIMING_PRESETS[pace];
  return preset.introFrames + preset.sceneFrames * TOTAL_SCENES + preset.outroFrames;
};

export const BRANCHBOX_TEASER_DURATION = presetDuration('full');
export const BRANCHBOX_TEASER_SOCIAL_DURATION = presetDuration('social');

const BRAND = {
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

const getLayoutPreset = (format: Format): LayoutPreset => {
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

const buildScenes = (stack: Stack): Scene[] => {
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

const BranchBoxLogo: React.FC<{size: number}> = ({size}) => {
  return (
    <div
      style={{
        width: size,
        height: size,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
      }}
    >
      <svg viewBox="0 0 32 32" width={size} height={size} fill="none">
        <rect x="2" y="2" width="28" height="28" rx="4" stroke={BRAND.accentBright} strokeWidth="2" />
        <path d="M8 10h6" stroke={BRAND.accentBright} strokeWidth="2" strokeLinecap="round" />
        <path d="M8 16h10" stroke={BRAND.accentBright} strokeWidth="2" strokeLinecap="round" />
        <path d="M8 22h6" stroke={BRAND.accentBright} strokeWidth="2" strokeLinecap="round" />
        <circle cx="22" cy="10" r="2" fill={BRAND.success} />
        <circle cx="22" cy="16" r="2" fill={BRAND.accentBright} />
        <circle cx="22" cy="22" r="2" fill={BRAND.cyan} />
      </svg>
    </div>
  );
};

export const BranchBoxTeaser: React.FC<BranchBoxTeaserProps> = ({stack, format = 'landscape', pace}) => {
  const frame = useCurrentFrame();
  const {fps} = useVideoConfig();
  const scenes = buildScenes(stack);
  const layout = getLayoutPreset(format);
  const activePace: Pace = pace ?? (format === 'landscape' ? 'full' : 'social');
  const timing = TIMING_PRESETS[activePace];
  const introFrames = timing.introFrames;
  const sceneFrames = timing.sceneFrames;
  const totalDuration = presetDuration(activePace);
  const introSpring = spring({
    fps,
    frame,
    config: {damping: 110, stiffness: 140},
  });

  const inIntro = frame < introFrames;
  const sceneSectionStart = introFrames;
  const sceneSectionEnd = sceneSectionStart + sceneFrames * TOTAL_SCENES;
  const inScenes = frame >= sceneSectionStart && frame < sceneSectionEnd;
  const inOutro = frame >= sceneSectionEnd;

  let activeSceneIndex = 0;
  let sceneFrame = 0;
  if (inScenes) {
    const relative = frame - sceneSectionStart;
    activeSceneIndex = Math.min(TOTAL_SCENES - 1, Math.floor(relative / sceneFrames));
    sceneFrame = relative - activeSceneIndex * sceneFrames;
  }

  const activeScene = scenes[activeSceneIndex];
  const sceneCommand =
    format === 'landscape'
      ? activeScene.command
      : format === 'vertical'
        ? activeScene.verticalCommand ?? activeScene.mobileCommand ?? activeScene.command
        : activeScene.mobileCommand ?? activeScene.command;
  const sceneTitle = format === 'landscape' ? activeScene.title : activeScene.mobileTitle ?? activeScene.title;
  const progressLabel =
    format === 'landscape'
      ? activeScene.progressLabel
      : activeScene.mobileProgressLabel ?? activeScene.progressLabel;
  const sceneOutput = format === 'landscape' ? activeScene.output : activeScene.mobileOutput;
  const commandTypingFrames =
    activePace === 'social'
      ? activeScene.socialCommandTypingFrames ?? layout.commandTypingFrames
      : layout.commandTypingFrames;
  const outputStartFrame =
    activePace === 'social'
      ? activeScene.socialOutputStartFrame ?? layout.outputStartFrame
      : layout.outputStartFrame;
  const outputRevealEveryFrames =
    activePace === 'social'
      ? activeScene.socialOutputRevealEveryFrames ?? layout.outputRevealEveryFrames
      : layout.outputRevealEveryFrames;
  const socialCommandStartChars =
    activePace === 'social'
      ? Math.min(sceneCommand.length, Math.max(14, Math.floor(sceneCommand.length * 0.34)))
      : 0;
  const typedChars = Math.floor(
    interpolate(sceneFrame, [0, commandTypingFrames], [socialCommandStartChars, sceneCommand.length], {
      extrapolateLeft: 'clamp',
      extrapolateRight: 'clamp',
    })
  );
  const socialOutputBaseline =
    activePace === 'social' && sceneFrame >= outputStartFrame
      ? 1
      : 0;
  const visibleOutputCount = Math.max(
    0,
    Math.min(
      sceneOutput.length,
      Math.floor((sceneFrame - outputStartFrame) / outputRevealEveryFrames) + socialOutputBaseline
    )
  );
  const visibleOutput = sceneOutput.slice(
    0,
    Math.min(layout.maxVisibleOutputLines, visibleOutputCount)
  );
  const cursorVisible = Math.floor(sceneFrame / layout.cursorBlinkFrames) % 2 === 0;
  const repoLabel = format === 'landscape' ? 'github.com/branchbox/branchbox' : 'github.com/branchbox';

  const sceneFadeFrames = activePace === 'social' ? 6 : 14;
  const sceneEdgeOpacity = activePace === 'social' ? 0.55 : 0;
  const sceneOpacity = interpolate(
    sceneFrame,
    [0, sceneFadeFrames, sceneFrames - sceneFadeFrames, sceneFrames],
    [sceneEdgeOpacity, 1, 1, sceneEdgeOpacity],
    {
      extrapolateLeft: 'clamp',
      extrapolateRight: 'clamp',
    }
  );
  const outroProgress = inOutro
    ? interpolate(frame, [sceneSectionEnd, totalDuration], [0, 1], {
        extrapolateLeft: 'clamp',
        extrapolateRight: 'clamp',
      })
    : 0;
  const compactSceneHeader = format === 'vertical' && inScenes;
  const heroStackGap = compactSceneHeader ? 8 : layout.heroGap;
  const heroHeadingSize = compactSceneHeader ? Math.round(layout.headingSize * 0.78) : layout.headingSize;
  const showSubheading = !(format === 'vertical' && inScenes);
  const badgeScale = compactSceneHeader ? 0.9 : 1;

  return (
    <AbsoluteFill
      style={{
        padding: layout.rootPadding,
        background: [
          'radial-gradient(circle at 15% 24%, rgba(139, 92, 246, 0.28) 0%, rgba(139, 92, 246, 0) 54%)',
          'radial-gradient(circle at 92% 12%, rgba(34, 211, 238, 0.22) 0%, rgba(34, 211, 238, 0) 42%)',
          `linear-gradient(130deg, ${BRAND.bgPrimary} 0%, ${BRAND.bgSecondary} 48%, ${BRAND.bgTertiary} 100%)`,
        ].join(','),
        color: BRAND.textPrimary,
        fontFamily: '"Inter", "Segoe UI", sans-serif',
      }}
    >
      <div
        style={{
          position: 'absolute',
          top: 0,
          right: 0,
          width: format === 'vertical' ? 430 : 560,
          height: format === 'vertical' ? 430 : 560,
          borderRadius: '50%',
          background: `radial-gradient(circle, rgba(139, 92, 246, 0.22) 0%, rgba(139, 92, 246, 0) 72%)`,
          transform: 'translate(30%, -30%)',
        }}
      />
      <div style={{display: 'flex', flexDirection: 'column', gap: heroStackGap}}>
        <div
          style={{
            transform: `translateY(${interpolate(introSpring, [0, 1], [16, 0])}px)`,
            opacity: introSpring,
          }}
        >
          <div
            style={{
              display: 'inline-flex',
              alignItems: 'center',
              gap: 12,
              borderRadius: 999,
              padding: '8px 14px',
              border: `1px solid ${BRAND.accentGlow}`,
              background: 'rgba(10, 10, 28, 0.72)',
              fontSize: layout.eyebrowSize,
              fontWeight: 600,
              color: BRAND.textSecondary,
              marginBottom: layout.topGap,
              transform: `scale(${badgeScale})`,
              transformOrigin: 'left center',
            }}
          >
            <BranchBoxLogo size={layout.logoSize} />
            <span>BranchBox</span>
          </div>
          <h1
            style={{
              margin: 0,
              fontSize: heroHeadingSize,
              lineHeight: 1,
              letterSpacing: -1.8,
              fontWeight: 700,
            }}
          >
            BranchBox in 50 Seconds
          </h1>
          {showSubheading && (
            <p
              style={{
                margin: '12px 0 0 0',
                fontSize: layout.subheadingSize,
                color: BRAND.textSecondary,
                maxWidth: layout.introTitleMaxWidth,
              }}
            >
              Spin up parallel feature environments with real CLI output
            </p>
          )}
        </div>

        {inIntro && (
          <div
            style={{
              borderRadius: 28,
              border: `1px solid ${BRAND.accentGlow}`,
              background: `linear-gradient(145deg, rgba(20, 20, 50, 0.78), ${BRAND.card})`,
              padding: format === 'vertical' ? '24px 24px' : '32px 38px',
              boxShadow: '0 18px 42px rgba(2, 6, 23, 0.45)',
            }}
          >
            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 14,
                fontSize: layout.cardTitleSize,
                fontWeight: 700,
              }}
            >
              <BranchBoxLogo size={layout.logoSize + 6} />
              <span>Built from a real BranchBox workflow capture</span>
            </div>
            <div style={{marginTop: 14, fontSize: layout.cardBodySize, color: BRAND.textSecondary}}>
              Capture date: {CAPTURED_AT}
            </div>
            <div style={{marginTop: 14, fontSize: layout.cardBodySize, color: BRAND.textSecondary}}>
              Story: full setup, minimal mode, feature visibility, devcontainer sync
            </div>
          </div>
        )}

        {inScenes && (
          <div
            style={{
              borderRadius: 24,
              padding: layout.terminalPadding,
              backgroundColor: BRAND.cardStrong,
              border: `1px solid ${BRAND.accentGlow}`,
              boxShadow: `0 18px 36px rgba(0, 0, 0, 0.35), 0 0 40px ${BRAND.accentGlow}`,
              fontFamily:
                '"JetBrains Mono", "Berkeley Mono", "SFMono-Regular", Menlo, Consolas, "Liberation Mono", monospace',
              color: BRAND.textPrimary,
              display: 'flex',
              flexDirection: 'column',
              gap: layout.terminalGap,
              height: layout.terminalHeight,
              opacity: sceneOpacity,
            }}
          >
            <div
              style={{
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'center',
                borderBottom: `1px solid ${BRAND.accentGlow}`,
                paddingBottom: 12,
                fontSize: layout.terminalHeaderSize,
                color: BRAND.accentBright,
              }}
            >
              <span style={{display: 'flex', alignItems: 'center', gap: 8}}>
                <BranchBoxLogo size={layout.logoSize} />
                <span>branchbox/{stack}</span>
              </span>
              <span>{sceneTitle}</span>
            </div>
            <div style={{fontSize: layout.terminalCommandSize, color: BRAND.textPrimary, whiteSpace: 'pre-wrap'}}>
              <span style={{color: BRAND.warning}}>$</span> {sceneCommand.slice(0, typedChars)}
              {cursorVisible ? <span style={{color: BRAND.accentBright}}>▋</span> : null}
            </div>
            <div
              style={{
                display: 'flex',
                flexDirection: 'column',
                gap: 5,
                fontSize: layout.terminalBodySize,
                color: BRAND.textSecondary,
              }}
            >
              {visibleOutput.map((line, index) => (
                <div key={`${activeSceneIndex}-${index}`} style={{whiteSpace: 'pre-wrap'}}>
                  {line}
                </div>
              ))}
            </div>
            <div
              style={{
                marginTop: 'auto',
                borderTop: `1px solid ${BRAND.accentGlow}`,
                paddingTop: 14,
                display: 'flex',
                justifyContent: 'space-between',
                fontSize: layout.terminalFooterSize,
                color: BRAND.textDim,
              }}
            >
              <span>{progressLabel}</span>
              <span>{repoLabel}</span>
            </div>
          </div>
        )}

        {inOutro && (
          <div
            style={{
              borderRadius: 28,
              border: `1px solid ${BRAND.accentGlow}`,
              background: `linear-gradient(145deg, rgba(20, 20, 50, 0.78), ${BRAND.card})`,
              padding: format === 'vertical' ? '34px 34px' : '36px 40px',
              boxShadow: '0 20px 45px rgba(2, 6, 23, 0.45)',
              opacity: outroProgress,
              transform: `translateY(${interpolate(outroProgress, [0, 1], [16, 0])}px)`,
            }}
          >
            <div style={{fontSize: layout.cardTitleSize + 4, fontWeight: 700}}>
              Parallel features. Real environments.
            </div>
            <div style={{marginTop: 12, fontSize: layout.cardBodySize + 2, color: BRAND.textSecondary}}>
              Rendered from captured BranchBox CLI output.
            </div>
            <div style={{marginTop: 14, fontSize: layout.cardBodySize, color: BRAND.textSecondary}}>
              Run: <code>./scripts/remotion-demo.sh --stack {stack}</code>
            </div>
          </div>
        )}
      </div>
    </AbsoluteFill>
  );
};
