import type React from 'react';
import {Composition} from 'remotion';
import {
  BRANCHBOX_TEASER_DURATION,
  BRANCHBOX_TEASER_SOCIAL_DURATION,
  BranchBoxTeaser,
  type BranchBoxTeaserProps,
} from './teaser/BranchBoxTeaser';

const defaultProps: BranchBoxTeaserProps = {
  stack: 'rust',
};

export const RemotionRoot: React.FC = () => {
  return (
    <>
      <Composition
        id="BranchBoxTeaser"
        component={BranchBoxTeaser}
        width={1920}
        height={1080}
        fps={30}
        durationInFrames={BRANCHBOX_TEASER_DURATION}
        defaultProps={{...defaultProps, format: 'landscape', pace: 'full'}}
      />
      <Composition
        id="BranchBoxTeaserSquare"
        component={BranchBoxTeaser}
        width={1080}
        height={1080}
        fps={30}
        durationInFrames={BRANCHBOX_TEASER_SOCIAL_DURATION}
        defaultProps={{...defaultProps, format: 'square', pace: 'social'}}
      />
      <Composition
        id="BranchBoxTeaserVertical"
        component={BranchBoxTeaser}
        width={1080}
        height={1920}
        fps={30}
        durationInFrames={BRANCHBOX_TEASER_SOCIAL_DURATION}
        defaultProps={{...defaultProps, format: 'vertical', pace: 'social'}}
      />
    </>
  );
};
