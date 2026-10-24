import { useResetOnChange } from '../hooks/useResetOnChange';
import {
  cloneElement,
  isValidElement,
  useEffect,
  useState,
  type ReactElement,
  type ReactNode,
} from 'react';
import { HaloOrb } from './halo/HaloOrb';
import { useDaemonImage } from '../hooks/useDaemonImage';
import {
  orbEnergy,
  orbPixel,
  useAgentOrbStyle,
} from '../lib/orb-style-preferences';

export function refreshAgentAvatar(id: string) {
  window.dispatchEvent(new CustomEvent('agent-avatar-changed', { detail: id }));
}

export function AgentAvatar({
  id,
  name,
  size = 28,
  fallback,
  energy,
}: {
  id: string;
  name: string;
  size?: number;
  fallback?: ReactNode;
  energy?: number;
}) {
  const [revision, setRevision] = useState(0);
  const [failed, setFailed] = useState(false);
  const image = useDaemonImage(
    `/api/agents/${encodeURIComponent(id)}/avatar?v=${revision}`,
  );
  const orbStyle = useAgentOrbStyle(id);
  const defaultOrb = (
    <HaloOrb
      accent={orbStyle.color}
      energy={energy ?? orbEnergy(orbStyle.motion)}
      pixel={orbPixel(orbStyle.grain)}
    />
  );
  const avatarFallback =
    fallback && isValidElement(fallback) && fallback.type === HaloOrb
      ? cloneElement(
          fallback as ReactElement<{
            accent?: string;
            energy?: number;
            pixel?: number;
          }>,
          {
            accent: orbStyle.color,
            energy: energy ?? orbEnergy(orbStyle.motion),
            pixel: orbPixel(orbStyle.grain),
          },
        )
      : (fallback ?? defaultOrb);
  const useFallback = failed || image.failed || image.src === null;
  useResetOnChange([id], () => setFailed(false));
  useEffect(() => {
    const refresh = (event: Event) => {
      if ((event as CustomEvent<string>).detail === id) {
        setFailed(false);
        setRevision(Date.now());
      }
    };
    window.addEventListener('agent-avatar-changed', refresh);
    return () => window.removeEventListener('agent-avatar-changed', refresh);
  }, [id]);
  return (
    <span
      className="agent-avatar inline-flex shrink-0 items-center justify-center overflow-hidden font-semibold"
      style={{ width: size, height: size }}
      data-default={useFallback || undefined}
      title={useFallback ? name : undefined}
    >
      {/* Use the same presence shader as the design when no image is stored. */}
      {useFallback ? (
        avatarFallback
      ) : (
        <img
          key={`${id}:${revision}`}
          src={image.src ?? undefined}
          alt=""
          className="h-full w-full object-cover"
          onError={() => setFailed(true)}
        />
      )}
    </span>
  );
}
