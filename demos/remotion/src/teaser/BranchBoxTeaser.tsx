import type React from 'react';
import {AbsoluteFill, interpolate, spring, useCurrentFrame, useVideoConfig} from 'remotion';
import {
  BRAND,
  buildScenes,
  CAPTURED_AT,
  getLayoutPreset,
  presetDuration,
  TIMING_PRESETS,
  TOTAL_SCENES,
  type BranchBoxTeaserProps,
  type Pace,
} from './teaserData';

const SOCIAL_COMMAND_PRETYPED_MIN_CHARS = 14;
const SOCIAL_COMMAND_PRETYPED_RATIO = 0.34;

export {BRANCHBOX_TEASER_DURATION, BRANCHBOX_TEASER_SOCIAL_DURATION} from './teaserData';
export type {BranchBoxTeaserProps} from './teaserData';

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
      ? Math.min(
          sceneCommand.length,
          Math.max(
            SOCIAL_COMMAND_PRETYPED_MIN_CHARS,
            Math.floor(sceneCommand.length * SOCIAL_COMMAND_PRETYPED_RATIO)
          )
        )
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
