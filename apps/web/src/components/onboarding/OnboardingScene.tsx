import type { CSSProperties, ReactNode } from 'react';
import { HaloOrb, type HaloPortrait } from '../halo/HaloOrb';

export type SceneStep =
  | 'welcome'
  | 'existing'
  | 'model'
  | 'you'
  | 'access'
  | 'voice';

const KEYART = '/anima-keyart.webp';
const FACE: HaloPortrait = {
  src: KEYART,
  reveal: 1,
  zoom: 1,
  cx: 0.34,
  cy: 0.45,
  contrast: 1.3,
  mask: 1,
};
const crop = (
  zoom: number,
  cx: number,
  cy: number,
  contrast = 1.3,
): HaloPortrait => ({
  src: KEYART,
  reveal: 1,
  zoom,
  cx,
  cy,
  contrast,
  mask: 0,
});

/** Places a node in the 1408 x 868 design stage, in design pixels. */
const at = (x: number, y: number, w?: number, h?: number): CSSProperties =>
  ({
    '--x': x,
    '--y': y,
    ...(w === undefined ? {} : { '--w': w }),
    ...(h === undefined ? {} : { '--h': h }),
  }) as CSSProperties;

/** Selection handles: square marks on the corners of the live item. */
function Handles() {
  return (
    <>
      {(['tl', 'tr', 'bl', 'br'] as const).map((corner) => (
        <span key={corner} className="ob-handle" data-corner={corner} />
      ))}
    </>
  );
}

/** A framed window onto anima: the orb dissolving into the portrait. */
function Window({
  portrait,
  className = '',
  style,
  energy = 0.4,
  pixel = 2,
}: {
  portrait: HaloPortrait;
  className?: string;
  style?: CSSProperties;
  energy?: number;
  pixel?: number;
}) {
  return (
    <div className={`ob-window ${className}`} style={style}>
      <HaloOrb
        portrait={portrait}
        energy={energy}
        pixel={pixel}
        mix={0.1}
        className="ob-window__orb"
      />
      <Handles />
    </div>
  );
}

/**
 * A lens onto the orb's own face: the portrait is drawn at the orb's size and
 * position, then cropped, so the lens lines up with the orb behind it.
 */
function Lens({
  rect,
  face,
  energy,
  accent = false,
  className = '',
}: {
  rect: [x: number, y: number, w: number, h: number];
  face: [x: number, y: number, size: number];
  energy: number;
  accent?: boolean;
  className?: string;
}) {
  return (
    <div
      className={`ob-window ob-lens${accent ? ' ob-window--accent' : ''} ${className}`}
      style={at(...rect)}
    >
      <div className="ob-lens__clip">
        <HaloOrb
          portrait={FACE}
          energy={energy}
          pixel={2}
          mix={0.1}
          className="ob-stage__orb"
          style={at(face[0], face[1], face[2], face[2])}
        />
      </div>
      <Handles />
    </div>
  );
}

function Caption({
  x,
  y,
  index,
  children,
  hot = false,
}: {
  x: number;
  y: number;
  index: string;
  children: ReactNode;
  hot?: boolean;
}) {
  return (
    <span className="ob-caption" style={at(x, y)}>
      <i>{index}</i>
      <b data-hot={hot || undefined}>{children}</b>
    </span>
  );
}

function Tag({ children }: { children: ReactNode }) {
  return (
    <span className="ob-tag">
      <i aria-hidden="true" />
      {children}
    </span>
  );
}

/**
 * The full-width stage behind the setup card. Each step frames anima a
 * little differently: the face on arrival, the orb while it thinks, a boundary
 * around the home folder, the mouth when it speaks.
 */
export function OnboardingScene({
  step,
  modelLabel,
  sourceLabel,
  rootPath,
  children,
}: {
  step: SceneStep;
  modelLabel: string;
  sourceLabel: string;
  rootPath: string;
  children?: ReactNode;
}) {
  const stageOrb = (
    rect: [number, number, number],
    energy: number,
    extra = '',
  ) => (
    <HaloOrb
      energy={energy}
      mix={0.18}
      className={`ob-stage__orb ${extra}`}
      style={at(rect[0], rect[1], rect[2], rect[2])}
    />
  );
  return (
    <div className="onboarding__scene" data-step={step}>
      {(step === 'welcome' || step === 'existing') && (
        <div className="ob-stage" aria-hidden="true">
          <i className="ob-stage__glow" style={at(350, -54, 940, 940)} />
          {stageOrb([500, 96, 640], 0.45)}
          <Lens
            rect={[622, 324, 406, 160]}
            face={[-122, -228, 640]}
            energy={0.45}
          />
          <Caption x={622} y={496} index="01" hot>
            ✦ HELLO
          </Caption>
          <Window
            portrait={crop(3.6, 0.415, 0.42)}
            className="ob-window--accent"
            style={at(968, 174, 150, 170)}
          />
          <Caption x={1126} y={174} index="02">
            4×
          </Caption>
          <span className="ob-mono" style={at(176, 812)}>
            Two minutes. No sign-up.
          </span>
        </div>
      )}
      {step === 'model' && (
        <div className="ob-stage" aria-hidden="true">
          <div className="ob-frame" style={at(589, 180, 500, 500)}>
            <HaloOrb energy={0.8} mix={0.18} className="ob-frame__orb" />
          </div>
          <Window
            portrait={crop(3.2, 0.41, 0.42)}
            className="ob-window--accent"
            style={at(1069, 140, 180, 140)}
          />
          {modelLabel && (
            <div className="ob-note" style={at(968, 610, 220)}>
              <span className="onboarding__kicker">Thinks with</span>
              <strong>{modelLabel}</strong>
              {sourceLabel && <span>{sourceLabel}</span>}
            </div>
          )}
          <span className="ob-mono" style={at(96, 812)}>
            Keys stay on this computer.
          </span>
        </div>
      )}
      {step === 'you' && (
        <div className="ob-stage">
          <div className="ob-deco" aria-hidden="true">
            <div className="ob-frame" style={at(589, 180, 500, 500)}>
              <HaloOrb energy={0.8} mix={0.18} className="ob-frame__orb" />
            </div>
            <Window
              portrait={crop(1.8, 0.45, 0.43, 1.15)}
              className="ob-window--accent"
              style={at(1104, 164, 144, 224)}
              energy={0.35}
              pixel={1.5}
            />
          </div>
          {children}
        </div>
      )}
      {step === 'access' && (
        <div className="ob-stage">
          <div className="ob-deco" aria-hidden="true">
            <div
              className="ob-frame ob-frame--accent"
              style={at(640, 200, 280, 420)}
            >
              <HaloOrb
                energy={0.6}
                mix={0.18}
                zoom={0.7}
                className="ob-frame__orb"
              />
              <Handles />
              <Tag>
                Home folder
                {rootPath && (
                  <span className="ob-tag__path">
                    {' · '}
                    {rootPath.replace(/^\\\\\?\\/, '')}
                  </span>
                )}
              </Tag>
            </div>
            <Window
              portrait={crop(4.6, 0.57, 0.65, 1.45)}
              className="ob-window--accent"
              style={at(936, 128, 300, 104)}
              energy={0.6}
              pixel={3}
            />
            <p className="ob-mono ob-outside" style={at(640, 632)}>
              Everything else on this computer
              <br />
              can’t be opened unless you add it.
            </p>
          </div>
          {children}
        </div>
      )}
      {step === 'voice' && (
        <div className="ob-stage">
          <div className="ob-deco" aria-hidden="true">
            <i className="ob-stage__glow" style={at(-90, 0, 900, 900)} />
            {stageOrb([60, 150, 600], 0.75)}
            <Lens
              rect={[282, 576, 204, 84]}
              face={[-222, -426, 600]}
              energy={0.75}
              accent
            />
          </div>
          {children}
        </div>
      )}
    </div>
  );
}
