import { useEffect, useMemo, useState } from 'react';
import type { ImageOperation } from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';
import { revealInvisible } from '../../lib/skills';
import { ImageArtifactLink } from '../artifacts/WorkspaceArtifactPreview';

export interface ImageGalleryProps {
  agentId: string;
  sessionId: string;
  clearGeneration: number;
  refreshKey?: unknown;
  enabled: boolean;
  knownImageWork?: boolean;
}

type ImageLedgerState = {
  kind: 'ready' | 'unsupported' | 'unavailable';
  operations: ImageOperation[];
};

function observationLabel(observation: ImageOperation['observation']): string {
  if (!observation) return 'Saved image bytes have not been checked by this backend.';
  switch (observation.state) {
    case 'bytes_matched': return `Saved bytes matched at ${new Date(observation.observedAtMs).toLocaleString()}.`;
    case 'unavailable': return 'Saved image bytes could not be verified. The recorded request status is unchanged.';
    case 'not_checked': return observation.reason === 'reader_busy' ? 'Saved image bytes were not checked because the readers are busy.' : 'Saved image bytes were not checked within this read limit.';
  }
}

export function ImageGallery(props: ImageGalleryProps) {
  return <ImageGalleryInner key={JSON.stringify([props.agentId, props.sessionId, props.clearGeneration, props.enabled])} {...props} />;
}

function ImageGalleryInner({ agentId, sessionId, clearGeneration, refreshKey, enabled, knownImageWork = false }: ImageGalleryProps) {
  const [ledger, setLedger] = useState<ImageLedgerState>({ kind: 'ready', operations: [] });
  const [attempt, setAttempt] = useState(0);
  const request = useMemo(() => ({ agentId, sessionId, clearGeneration, refreshKey, enabled, attempt }), [agentId, sessionId, clearGeneration, refreshKey, enabled, attempt]);
  useEffect(() => {
    if (!request.enabled) return;
    let active = true;
    const controller = new AbortController();
    let timer: number | undefined;
    const load = async () => {
      try {
        const inventory = await daemon.capabilities();
        if (!active) return;
        if (inventory.runtime?.images.ledgerVersion !== 1) {
          setLedger((previous) => ({ kind: 'unsupported', operations: previous.operations }));
          return;
        }
        const page = await daemon.images.list(request.agentId, request.sessionId, { signal: controller.signal });
        if (!active) return;
        if (page.clearGeneration !== request.clearGeneration) {
          setLedger((previous) => ({ kind: 'unavailable', operations: previous.operations }));
          return;
        }
        setLedger({ kind: 'ready', operations: page.operations });
        if (page.operations.some((operation) => operation.status === 'dispatching')) timer = window.setTimeout(() => void load(), 2000);
      } catch {
        if (active) setLedger((previous) => ({ kind: 'unavailable', operations: previous.operations }));
      }
    };
    void load();
    return () => { active = false; controller.abort(); window.clearTimeout(timer); };
  }, [request]);
  const { operations } = ledger;
  const hasKnownWork = knownImageWork || operations.length > 0;
  if (!enabled || (!operations.length && (ledger.kind === 'ready' || !hasKnownWork))) return null;
  const refresh = <button type="button" className="studio-tool-button" onClick={() => setAttempt((value) => value + 1)}>Refresh images</button>;
  const notice = ledger.kind === 'unsupported'
    ? 'The running backend does not advertise image history support. Update the backend to inspect this image work.'
    : ledger.kind === 'unavailable'
      ? 'Image versions could not be loaded. Refresh images to try again. The image request will not be resent.'
      : null;
  if (!operations.length) return <div className="mx-4 my-2 flex items-center gap-3 text-xs text-ink-3"><p role="status">{notice}</p>{refresh}</div>;
  const savedImages = operations.flatMap((operation) => operation.version ? [{ version: operation.version, observation: operation.observation }] : []);
  const versions = savedImages.map((image) => image.version);
  return <section aria-label="Chat images" className="mx-4 my-2 max-h-56 overflow-auto rounded-xl border border-line bg-panel p-3 text-sm">
    <div className="flex items-center justify-between gap-3"><h2 className="font-semibold">Images</h2>{refresh}</div>
    {notice && <p role="status">{notice}</p>}
    {versions.length > 0 && <ol className="mt-2 flex max-h-32 gap-2 overflow-auto" aria-label="Saved image versions">
      {savedImages.map(({ version, observation }, index) => <li key={version.id} className="min-w-32 max-w-56 rounded-lg border border-line p-2">
        <ImageArtifactLink versions={versions} versionId={version.id} label={`Open image version ${index + 1}`}>
          <span className="block text-xs text-ink-3">Version {index + 1}{version.parentAttachmentId ? ' · Edited' : ''}</span>
          <span className="block break-words">{revealInvisible(version.attachment.name).text}</span>
        </ImageArtifactLink>
        <p className="mt-1 text-xs text-ink-3">{observationLabel(observation)}</p>
      </li>)}
    </ol>}
    {versions.length > 0 && <p className="mt-2 text-xs text-ink-3">Checks cover saved bytes only. Visual content has not been verified.</p>}
    {operations.filter((operation) => operation.status !== 'succeeded').map((operation) => <p key={operation.id} role="status" className="mt-2">
      {operation.status === 'dispatching' ? 'Creating image…' : operation.status === 'outcome_unknown' ? 'The image request outcome is unknown. The provider may charge; it will not be retried automatically.' : 'The image request failed. Cost is unknown; the provider may charge.'}
    </p>)}
  </section>;
}
