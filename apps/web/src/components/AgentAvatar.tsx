import { useEffect, useState, type ReactNode } from 'react';
import { HaloOrb } from './halo/HaloOrb';

export function refreshAgentAvatar(id: string) {
  window.dispatchEvent(new CustomEvent('agent-avatar-changed', { detail: id }));
}

export function AgentAvatar({
  id,
  name,
  size = 28,
  fallback = <HaloOrb />,
}: {
  id: string;
  name: string;
  size?: number;
  fallback?: ReactNode;
}) {
  const [revision, setRevision] = useState(0);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    setFailed(false);
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
      data-default={failed || undefined}
      title={failed ? name : undefined}
    >
      {/* Use the same presence shader as the design when no image is stored. */}
      {failed ? (
        fallback
      ) : (
        <img
          key={`${id}:${revision}`}
          src={`/api/agents/${encodeURIComponent(id)}/avatar?v=${revision}`}
          alt=""
          className="h-full w-full object-cover"
          onError={() => setFailed(true)}
        />
      )}
    </span>
  );
}
