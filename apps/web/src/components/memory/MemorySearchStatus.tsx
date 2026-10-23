import { useLayoutEffect, useState, useSyncExternalStore } from 'react';
import type { MemoryReadiness } from '@animaos/sdk';

import { daemon } from '../../lib/daemon-api';
import { useLatest } from '../../hooks/useLatest';
import { useRenderScope } from '../../hooks/useRenderScope';

interface Props {
  agentId: string;
  online: boolean;
  epoch: number;
}

interface Snapshot {
  search: string;
  /** "External memory: <id> — ready"; absent for the default `none`. */
  externalMemory: string | null;
  enabled: boolean;
  provider: string;
  model: string;
  dimension: number;
  vectorCount: number;
  persisted: boolean;
  totalChecks: number;
  passedChecks: number;
}

type Boundary = ReturnType<typeof useRenderScope>;

// The SDK readiness method has no abort signal. An unmounted panel cannot free
// its read: only the actual settlement admits another check on this page.
let pendingRead: Promise<MemoryReadiness> | null = null;
const ownerListeners = new Set<() => void>();

function subscribeOwner(listener: () => void) {
  ownerListeners.add(listener);
  return () => ownerListeners.delete(listener);
}

function ownerBusy() {
  return pendingRead !== null;
}

function notifyOwner() {
  for (const listener of ownerListeners) listener();
}

function reserveRead(request: Promise<MemoryReadiness>) {
  pendingRead = request;
  notifyOwner();
}

function releaseRead(request: Promise<MemoryReadiness>) {
  if (pendingRead !== request) return;
  pendingRead = null;
  notifyOwner();
}

function record(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function count(value: unknown): value is number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;
}

// Only short configuration identifiers are displayable. In particular, paths,
// URLs, controls, and the diagnostic storageFile never enter rendered state.
function name(value: unknown): value is string {
  return (
    typeof value === 'string' &&
    value.length > 0 &&
    value.length <= 80 &&
    /^[A-Za-z0-9][A-Za-z0-9._:-]*$/.test(value)
  );
}

function modelName(value: unknown): value is string {
  return (
    name(value) ||
    (typeof value === 'string' &&
      value.length > 0 &&
      value.length <= 80 &&
      /^[A-Za-z0-9][A-Za-z0-9._-]*\/[A-Za-z0-9][A-Za-z0-9._:-]*$/.test(value))
  );
}

const FALLBACK_GENERIC = 'the search model could not load';

// The daemon leaves paths and URLs out of the reason; anything that still
// looks like one, or carries controls, shows the generic reason instead.
function fallbackReason(
  value: unknown,
  generic: string = FALLBACK_GENERIC,
): string {
  const unsafe = (character: string) => {
    const code = character.charCodeAt(0);
    return code < 32 || code === 127 || character === '/' || character === '\\';
  };
  return typeof value === 'string' &&
    value.trim().length > 0 &&
    value.length <= 200 &&
    ![...value].some(unsafe)
    ? value
    : generic;
}

// The search model loads in the background after boot; `null` is malformed.
function searchStatus(embeddings: Record<string, unknown>): string | null {
  switch (embeddings.status) {
    case 'downloading':
      return 'Downloading the search model…';
    case 'rebuilding':
      return count(embeddings.done) &&
        count(embeddings.total) &&
        embeddings.done <= embeddings.total
        ? `Rebuilding search (${embeddings.done}/${embeddings.total})`
        : null;
    case 'ready':
      return 'Search ready';
    case 'fallback':
      return `Basic search: ${fallbackReason(embeddings.reason)}`;
    case 'off':
      return 'Basic search: vector search is off';
    default:
      return null;
  }
}

// The external memory provider beside the local store. `none` shows nothing;
// a malformed entry shows nothing rather than failing the whole status.
function externalMemoryStatus(value: unknown): string | null {
  const provider = record(value);
  if (!provider || !name(provider.id) || provider.id === 'none') return null;
  switch (provider.status) {
    case 'ready':
      return `External memory: ${provider.id} — ready`;
    case 'unavailable':
      return `External memory: ${provider.id} — unavailable: ${fallbackReason(provider.reason, 'no reason given')}`;
    default:
      return null;
  }
}

function projectReadiness(value: unknown): Snapshot | null {
  const top = record(value);
  const embeddings = record(top?.embeddings);
  const evaluation = record(top?.evaluation);
  const search = embeddings ? searchStatus(embeddings) : null;
  if (
    !embeddings ||
    search === null ||
    !evaluation ||
    typeof embeddings.enabled !== 'boolean' ||
    !name(embeddings.provider) ||
    !modelName(embeddings.model) ||
    !count(embeddings.dimension) ||
    !count(embeddings.vectorCount) ||
    typeof embeddings.persisted !== 'boolean' ||
    !count(evaluation.totalChecks) ||
    !count(evaluation.passedChecks) ||
    evaluation.passedChecks > evaluation.totalChecks
  ) {
    return null;
  }
  return {
    search,
    externalMemory: externalMemoryStatus(top?.externalProvider),
    enabled: embeddings.enabled,
    provider: embeddings.provider,
    model: embeddings.model,
    dimension: embeddings.dimension,
    vectorCount: embeddings.vectorCount,
    persisted: embeddings.persisted,
    totalChecks: evaluation.totalChecks,
    passedChecks: evaluation.passedChecks,
  };
}

export function MemorySearchStatus({ agentId, online, epoch }: Props) {
  const boundary = useRenderScope(agentId, online, epoch);
  const boundaryRef = useLatest<Boundary | null>(boundary);
  const [result, setResult] = useState<{
    boundary: Boundary;
    snapshot: Snapshot | null;
  } | null>(null);
  const busy = useSyncExternalStore(subscribeOwner, ownerBusy, ownerBusy);

  useLayoutEffect(() => {
    return () => {
      if (boundaryRef.current === boundary) boundaryRef.current = null;
    };
  }, [boundary, boundaryRef]);

  const visible = result?.boundary === boundary ? result.snapshot : undefined;

  const check = () => {
    const owner = boundaryRef.current;
    if (!online || owner !== boundary || pendingRead !== null) {
      return;
    }
    setResult(null);
    let request: Promise<MemoryReadiness>;
    try {
      request = daemon.memoryReadiness();
    } catch {
      setResult({ boundary: owner, snapshot: null });
      return;
    }
    reserveRead(request);
    const settle = (snapshot: Snapshot | null) => {
      try {
        if (boundaryRef.current === owner) {
          setResult({ boundary: owner, snapshot });
        }
      } finally {
        releaseRead(request);
      }
    };
    void request.then(
      (value) => {
        let snapshot: Snapshot | null = null;
        try {
          snapshot = projectReadiness(value);
        } catch {
          // The response is diagnostic data; keep failures generic.
        }
        settle(snapshot);
      },
      () => settle(null),
    );
  };

  return (
    <section className="memory-section" aria-label="Memory search status">
      <h3>Memory search status</h3>
      <p className="memory-meta">
        Check a daemon-wide vector-index snapshot. These are not counts of this
        companion’s saved facts.
      </p>
      <button
        type="button"
        className="studio-tool-button"
        disabled={!online || busy}
        onClick={check}
      >
        Check memory search status
      </button>
      {busy && (
        <p className="memory-meta" role="status">
          A memory search status check is pending. Reload if it does not finish.
        </p>
      )}
      {visible === null && !busy && (
        <p className="memory-error" role="status">
          Memory search status is unavailable.
        </p>
      )}
      {visible && !busy && (
        <div aria-live="polite">
          <p>{visible.search}</p>
          {visible.externalMemory && <p>{visible.externalMemory}</p>}
          <p>
            Daemon-wide vector index: {visible.enabled ? 'enabled' : 'disabled'}
            .
          </p>
          <p>Configured provider: {visible.provider}.</p>
          <p>Configured model: {visible.model}.</p>
          <p>
            Dimension: {visible.dimension}. Vectors: {visible.vectorCount}.
          </p>
          <p>
            Disk cache: {visible.persisted ? 'configured' : 'not configured'}.
            This does not confirm a recent write or canonical memory durability.
          </p>
          {visible.totalChecks === 0 ? (
            <p>No local fixture checks reported.</p>
          ) : (
            <>
              <p>
                Local fixture checks: {visible.passedChecks} of{' '}
                {visible.totalChecks} passed.
              </p>
              {visible.passedChecks < visible.totalChecks && (
                <p>
                  {visible.totalChecks - visible.passedChecks} local fixture
                  checks failed.
                </p>
              )}
            </>
          )}
          <p>
            Saved memories and keyword search remain separate from vector
            indexing. This snapshot does not verify provider reachability or
            semantic answer quality.
          </p>
        </div>
      )}
    </section>
  );
}
