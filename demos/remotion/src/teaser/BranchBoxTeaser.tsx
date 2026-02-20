import type React from 'react';
import {AbsoluteFill, interpolate, spring, useCurrentFrame, useVideoConfig} from 'remotion';
import {
  BRAND,
  buildScenes,
  CAPTURED_AT,
  getStoryCopy,
  getLayoutPreset,
  presetDuration,
  SOCIAL_PAIN_HOOK_FRAMES,
  TIMING_PRESETS,
  TOTAL_SCENES,
  type BranchBoxTeaserProps,
  type Pace,
} from './teaserData';

const SOCIAL_COMMAND_PRETYPED_MIN_CHARS = 22;
const SOCIAL_COMMAND_PRETYPED_RATIO = 0.62;

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

export const BranchBoxTeaser: React.FC<BranchBoxTeaserProps> = ({
  stack,
  format = 'landscape',
  pace,
  audience = 'marketing',
}) => {
  const isTallFormat = format === 'vertical' || format === 'web-mobile';
  const frame = useCurrentFrame();
  const {fps} = useVideoConfig();
  const scenes = buildScenes(stack, audience);
  const story = getStoryCopy(audience, format);
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
  let sceneCommand = activeScene.command;
  if (format === 'vertical') {
    sceneCommand = activeScene.verticalCommand ?? activeScene.mobileCommand ?? activeScene.command;
  } else if (format !== 'landscape') {
    sceneCommand = activeScene.mobileCommand ?? activeScene.command;
  }
  const sceneTitle = format === 'landscape' ? activeScene.title : activeScene.mobileTitle ?? activeScene.title;
  const painHookText =
    format === 'landscape' ? activeScene.painHook : activeScene.mobilePainHook ?? activeScene.painHook;
  const sceneProofBadges = (
    format === 'landscape'
      ? activeScene.proofBadges
      : activeScene.mobileProofBadges ?? activeScene.proofBadges
  ) ?? [];
  const visibleProofBadges = sceneProofBadges.slice(0, format === 'landscape' ? 3 : 2);
  const progressLabel =
    format === 'landscape'
      ? activeScene.progressLabel
      : activeScene.mobileProgressLabel ?? activeScene.progressLabel;
  const sceneOutput = format === 'landscape' ? activeScene.output : activeScene.mobileOutput;
  const painHookFrames =
    activePace === 'social' && audience === 'marketing' && painHookText ? SOCIAL_PAIN_HOOK_FRAMES : 0;
  const showPainHook = painHookFrames > 0 && sceneFrame < painHookFrames;
  const sceneActionFrame = Math.max(0, sceneFrame - painHookFrames);
  const painHookOpacity =
    showPainHook && painHookFrames > 12
      ? interpolate(sceneFrame, [0, 12, painHookFrames - 10, painHookFrames], [0, 1, 1, 0], {
          extrapolateLeft: 'clamp',
          extrapolateRight: 'clamp',
        })
      : 0;
  const proofBadgesOpacity =
    activePace === 'social' && audience === 'marketing' && painHookFrames > 0
      ? interpolate(sceneFrame, [painHookFrames - 10, painHookFrames + 8], [0, 1], {
          extrapolateLeft: 'clamp',
          extrapolateRight: 'clamp',
        })
      : 1;
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
    interpolate(sceneActionFrame, [0, commandTypingFrames], [socialCommandStartChars, sceneCommand.length], {
      extrapolateLeft: 'clamp',
      extrapolateRight: 'clamp',
    })
  );
  const socialOutputBaseline =
    activePace === 'social' && sceneActionFrame >= outputStartFrame
      ? 1
      : 0;
  const visibleOutputCount = Math.max(
    0,
    Math.min(
      sceneOutput.length,
      Math.floor((sceneActionFrame - outputStartFrame) / outputRevealEveryFrames) + socialOutputBaseline
    )
  );
  const visibleOutput = sceneOutput.slice(
    0,
    Math.min(layout.maxVisibleOutputLines, visibleOutputCount)
  );
  const cursorVisible = Math.floor(sceneActionFrame / layout.cursorBlinkFrames) % 2 === 0;
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
  const useVerticalMarketingOutro = isTallFormat && audience === 'marketing';
  const compactSceneHeader = isTallFormat && inScenes;
  const suppressSceneHeading = isTallFormat && inScenes && activePace === 'social';
  const heroStackGap = compactSceneHeader ? 6 : layout.heroGap;
  const heroHeadingSize = compactSceneHeader ? Math.round(layout.headingSize * 0.64) : layout.headingSize;
  const showSubheading = !(isTallFormat && inScenes) && !(useVerticalMarketingOutro && inOutro);
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
          width: isTallFormat ? 430 : 560,
          height: isTallFormat ? 430 : 560,
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
          {!suppressSceneHeading && (
            <h1
              style={{
                margin: 0,
                fontSize: heroHeadingSize,
                lineHeight: 1,
                letterSpacing: -1.8,
                fontWeight: 700,
              }}
            >
              {story.heading}
            </h1>
          )}
          {showSubheading && (
            <p
              style={{
                margin: '12px 0 0 0',
                fontSize: layout.subheadingSize,
                color: BRAND.textSecondary,
                maxWidth: layout.introTitleMaxWidth,
              }}
            >
              {story.subheading}
            </p>
          )}
        </div>

        {inIntro && (
          <div
            style={{
              borderRadius: 28,
              border: `1px solid ${BRAND.accentGlow}`,
              background: `linear-gradient(145deg, rgba(20, 20, 50, 0.78), ${BRAND.card})`,
              padding: isTallFormat ? '24px 24px' : '32px 38px',
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
              <span>{story.introTitle}</span>
            </div>
            <div style={{marginTop: 14, fontSize: layout.cardBodySize, color: BRAND.textSecondary}}>
              Capture date: {CAPTURED_AT}
            </div>
            <div style={{marginTop: 14, fontSize: layout.cardBodySize, color: BRAND.textSecondary}}>
              {story.introStory}
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
            {showPainHook && painHookText ? (
              <div
                style={{
                  borderRadius: 14,
                  border: `1px solid rgba(245, 158, 11, 0.4)`,
                  background: 'rgba(35, 18, 6, 0.55)',
                  padding: format === 'landscape' ? '10px 12px' : '8px 10px',
                  display: 'flex',
                  alignItems: 'center',
                  gap: 8,
                  fontSize: format === 'landscape' ? layout.terminalBodySize : layout.terminalBodySize - 2,
                  color: '#fde68a',
                  opacity: painHookOpacity,
                }}
              >
                <span style={{color: BRAND.warning, fontWeight: 700}}>Pain:</span>
                <span>{painHookText}</span>
              </div>
            ) : null}
            {visibleProofBadges.length > 0 ? (
              <div
                style={{
                  display: 'flex',
                  gap: 8,
                  flexWrap: 'wrap',
                  opacity: proofBadgesOpacity,
                }}
              >
                {visibleProofBadges.map((badge) => (
                  <div
                    key={`${activeSceneIndex}-${badge}`}
                    style={{
                      borderRadius: 999,
                      border: `1px solid ${BRAND.accentGlow}`,
                      background: 'rgba(16, 16, 42, 0.78)',
                      padding: format === 'landscape' ? '5px 12px' : '4px 10px',
                      color: BRAND.cyan,
                      fontSize: format === 'landscape' ? layout.terminalBodySize - 3 : layout.terminalBodySize - 5,
                      whiteSpace: 'nowrap',
                    }}
                  >
                    {badge}
                  </div>
                ))}
              </div>
            ) : null}
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

        {inOutro &&
          (useVerticalMarketingOutro ? (
            <div
              style={{
                borderRadius: 24,
                border: `1px solid rgba(139, 92, 246, 0.42)`,
                background:
                  'linear-gradient(160deg, rgba(12, 12, 40, 0.96) 0%, rgba(8, 8, 28, 0.94) 100%)',
                padding: '28px 24px',
                boxShadow:
                  '0 18px 36px rgba(0, 0, 0, 0.35), 0 0 42px rgba(139, 92, 246, 0.2), inset 0 0 0 1px rgba(34, 211, 238, 0.1)',
                height: layout.terminalHeight,
                display: 'flex',
                flexDirection: 'column',
                gap: 14,
                opacity: interpolate(outroProgress, [0, 1], [0.94, 1], {
                  extrapolateLeft: 'clamp',
                  extrapolateRight: 'clamp',
                }),
                transform: `translateY(${interpolate(outroProgress, [0, 1], [10, 0])}px)`,
              }}
            >
              <div
                style={{
                  display: 'inline-block',
                  borderRadius: 999,
                  border: `1px solid rgba(139, 92, 246, 0.5)`,
                  background: 'rgba(14, 14, 44, 0.92)',
                  padding: '8px 14px',
                  color: BRAND.accentBright,
                  fontSize: layout.cardBodySize + 1,
                  fontWeight: 700,
                  marginBottom: 4,
                }}
              >
                {story.outroEyebrow}
              </div>
              <div
                style={{
                  fontSize: layout.cardTitleSize + 16,
                  lineHeight: 1.08,
                  letterSpacing: -1,
                  fontWeight: 800,
                  color: BRAND.textPrimary,
                  maxWidth: 940,
                  textShadow: '0 1px 14px rgba(56, 189, 248, 0.2)',
                }}
              >
                {story.outroTitle}
              </div>
              <div
                style={{
                  marginTop: 10,
                  borderRadius: 14,
                  border: `1px solid rgba(34, 211, 238, 0.45)`,
                  background:
                    'linear-gradient(140deg, rgba(139, 92, 246, 0.34), rgba(34, 211, 238, 0.24))',
                  padding: '14px 16px',
                  color: BRAND.textPrimary,
                  fontSize: layout.cardBodySize + 4,
                  fontWeight: 700,
                  lineHeight: 1.25,
                  wordBreak: 'break-word',
                }}
              >
                {story.outroLine1}
              </div>
              <div
                style={{
                  marginTop: 2,
                  fontSize: layout.cardBodySize + 2,
                  color: BRAND.textSecondary,
                  lineHeight: 1.3,
                }}
              >
                {story.outroLine2}
              </div>
              <div
                style={{
                  marginTop: 10,
                  lineHeight: 1.4,
                  borderRadius: 12,
                  border: `1px solid rgba(139, 92, 246, 0.38)`,
                  background: 'rgba(16, 16, 42, 0.9)',
                  padding: '12px 14px',
                  fontSize: layout.cardBodySize + 1,
                  color: BRAND.textPrimary,
                  whiteSpace: 'pre-line',
                }}
              >
                {'✓ Isolated worktree + branch per feature\n✓ No shared ports or env collisions\n✓ Safe devcontainer sync across active features'}
              </div>
              <div
                style={{
                  marginTop: 'auto',
                  borderTop: `1px solid ${BRAND.accentGlow}`,
                  paddingTop: 12,
                  display: 'flex',
                  justifyContent: 'space-between',
                  fontSize: layout.cardBodySize - 1,
                  color: BRAND.textDim,
                }}
              >
                <span>Share with your team</span>
                <span>github.com/branchbox</span>
              </div>
            </div>
          ) : (
            <div
              style={{
                borderRadius: 28,
                border: `1px solid ${BRAND.accentGlow}`,
                background: `linear-gradient(145deg, rgba(20, 20, 50, 0.78), ${BRAND.card})`,
                padding: isTallFormat ? '34px 34px' : '36px 40px',
                boxShadow: '0 20px 45px rgba(2, 6, 23, 0.45)',
                opacity: outroProgress,
                transform: `translateY(${interpolate(outroProgress, [0, 1], [16, 0])}px)`,
              }}
            >
              <div
                style={{
                  display: 'inline-block',
                  borderRadius: 999,
                  border: `1px solid ${BRAND.accentGlow}`,
                  background: 'rgba(10, 10, 28, 0.72)',
                  padding: '7px 12px',
                  color: BRAND.accentBright,
                  fontSize: layout.cardBodySize - 2,
                  fontWeight: 600,
                  marginBottom: 10,
                }}
              >
                {story.outroEyebrow}
              </div>
              <div style={{fontSize: layout.cardTitleSize + 4, fontWeight: 700}}>
                {story.outroTitle}
              </div>
              <div style={{marginTop: 12, fontSize: layout.cardBodySize + 2, color: BRAND.textSecondary}}>
                {story.outroLine1}
              </div>
              <div style={{marginTop: 14, fontSize: layout.cardBodySize, color: BRAND.textSecondary}}>
                {story.outroLine2}
              </div>
            </div>
          ))}
      </div>
    </AbsoluteFill>
  );
};
