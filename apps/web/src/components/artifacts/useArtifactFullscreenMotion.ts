import {
  useCallback,
  useLayoutEffect,
  useRef,
  useState,
  type RefObject,
} from 'react';

type Bounds = Pick<DOMRect, 'left' | 'top' | 'width' | 'height'>;
type Motion =
  | { kind: 'resize'; from: Bounds; to: Bounds; fullscreen: boolean }
  | { kind: 'close'; from: Bounds; opacity: string; onComplete(): void };

function keyframe(bounds: Bounds): Keyframe {
  return {
    left: `${bounds.left}px`,
    top: `${bounds.top}px`,
    width: `${bounds.width}px`,
    height: `${bounds.height}px`,
    right: 'auto',
    bottom: 'auto',
  };
}

export function useArtifactFullscreenMotion(
  layoutRef: RefObject<HTMLDivElement | null>,
  chatShare: number,
) {
  const slotRef = useRef<HTMLDivElement>(null);
  const [fullscreen, setFullscreen] = useState(false);
  const [motion, setMotion] = useState<Motion | null>(null);
  const targetFullscreen =
    motion?.kind === 'resize' ? motion.fullscreen : fullscreen;

  const transitionTo = useCallback(
    (next: boolean) => {
      const slot = slotRef.current;
      const layout = layoutRef.current;
      const reduced = window.matchMedia?.(
        '(prefers-reduced-motion: reduce)',
      ).matches;
      if (!slot || !layout || reduced || typeof slot.animate !== 'function') {
        setMotion(null);
        setFullscreen(next);
        return;
      }
      // Measure the current animated bounds too, so a quick reversal stays continuous.
      const from = slot.getBoundingClientRect();
      const container = layout.getBoundingClientRect();
      if (
        from.width <= 0 ||
        from.height <= 0 ||
        container.width <= 0 ||
        container.height <= 0
      ) {
        setMotion(null);
        setFullscreen(next);
        return;
      }
      const chatWidth =
        window.innerWidth < 768 ? 0 : (container.width * chatShare) / 100;
      const to = next
        ? {
            left: 0,
            top: 0,
            width: window.innerWidth,
            height: window.innerHeight,
          }
        : {
            left: container.left + chatWidth,
            top: container.top,
            width: container.width - chatWidth,
            height: container.height,
          };
      // Remain a fixed, isolated overlay until the return animation has finished.
      setFullscreen(true);
      setMotion({ kind: 'resize', from, to, fullscreen: next });
    },
    [chatShare, layoutRef],
  );

  const cancelFullscreen = useCallback(() => {
    setMotion(null);
    setFullscreen(false);
  }, []);

  const closeFullscreen = useCallback(
    (onComplete: () => void) => {
      const slot = slotRef.current;
      const reduced = window.matchMedia?.(
        '(prefers-reduced-motion: reduce)',
      ).matches;
      const from = slot?.getBoundingClientRect();
      if (
        !slot ||
        reduced ||
        typeof slot.animate !== 'function' ||
        !from ||
        from.width <= 0 ||
        from.height <= 0
      ) {
        cancelFullscreen();
        onComplete();
        return;
      }
      // Freeze any in-flight expansion at its visible bounds before fading it out.
      const opacity = getComputedStyle(slot).opacity || '1';
      setFullscreen(true);
      setMotion({ kind: 'close', from, opacity, onComplete });
    },
    [cancelFullscreen],
  );

  useLayoutEffect(() => {
    const slot = slotRef.current;
    if (!motion || !slot) return;
    let cancelled = false;
    const complete = () => {
      if (cancelled) return;
      setFullscreen(motion.kind === 'resize' ? motion.fullscreen : false);
      setMotion(null);
      if (motion.kind === 'close') motion.onComplete();
    };
    let animation: Animation;
    try {
      const frames =
        motion.kind === 'close'
          ? [
              {
                ...keyframe(motion.from),
                opacity: motion.opacity,
                transform: 'none',
              },
              {
                ...keyframe(motion.from),
                opacity: 0,
                transform: 'scale(0.985)',
              },
            ]
          : [keyframe(motion.from), keyframe(motion.to)];
      animation = slot.animate(frames, {
        duration: motion.kind === 'close' ? 220 : 280,
        easing: 'cubic-bezier(0.22, 1, 0.36, 1)',
        fill: 'both',
      });
    } catch {
      queueMicrotask(complete);
      return () => {
        cancelled = true;
      };
    }
    void animation.finished.then(complete, complete);
    return () => {
      cancelled = true;
      // Remove filled geometry only after React has committed the final layout.
      animation.cancel();
    };
  }, [motion]);

  return {
    slotRef,
    fullscreen,
    targetFullscreen,
    fullscreenAnimating: motion !== null,
    transitionTo,
    cancelFullscreen,
    closeFullscreen,
  };
}
