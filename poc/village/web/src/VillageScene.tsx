import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createVillageScene, type VillageSceneHandle } from './scene/scene';
import type { BreadPreference, NpcId, VillagePhase } from './types';

interface Props {
  paused: boolean;
  preference: BreadPreference;
  collected: boolean;
  approachSignal: number;
  approachNpc: NpcId;
  phase: VillagePhase;
  onNear: (near: NpcId | null) => void;
  onInteract: () => void;
}
export function VillageScene(props: Props) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const handle = useRef<VillageSceneHandle | null>(null);
  const callbacks = useRef(props);
  useLayoutEffect(() => {
    callbacks.current = props;
  });
  const [error, setError] = useState<string>();
  useEffect(() => {
    let mounted = true;
    queueMicrotask(() => {
      if (!mounted || !canvas.current) return;
      try {
        handle.current = createVillageScene(
          canvas.current,
          (near) => callbacks.current.onNear(near),
          () => callbacks.current.onInteract(),
        );
        handle.current.pause(callbacks.current.paused);
        handle.current.phase(callbacks.current.phase);
        handle.current.preference(
          callbacks.current.preference,
          callbacks.current.collected,
        );
      } catch {
        setError(
          'The village needs WebGL. Please enable browser hardware acceleration and reopen this page.',
        );
      }
    });
    return () => {
      mounted = false;
      handle.current?.dispose();
      handle.current = null;
    };
  }, []);
  useEffect(() => {
    handle.current?.pause(props.paused);
  }, [props.paused]);
  useEffect(() => {
    handle.current?.preference(props.preference, props.collected);
  }, [props.preference, props.collected]);
  useEffect(() => {
    handle.current?.phase(props.phase);
  }, [props.phase]);
  useEffect(() => {
    if (props.approachSignal > 0) handle.current?.approach(props.approachNpc);
  }, [props.approachSignal, props.approachNpc]);
  return (
    <>
      <canvas
        ref={canvas}
        className="world-canvas"
        tabIndex={0}
        aria-label="Explore Lantern Village with WASD, arrow keys, or click to walk. Meet Tavi at the shop and Mira by the supper table."
      />
      {error && (
        <p role="alert" className="world-error">
          {error}
        </p>
      )}
    </>
  );
}
