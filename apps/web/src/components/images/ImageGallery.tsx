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
}

export function ImageGallery(props: ImageGalleryProps) {
  return <ImageGalleryInner key={JSON.stringify([props.agentId, props.sessionId, props.clearGeneration, props.enabled])} {...props} />;
}

function ImageGalleryInner({ agentId, sessionId, clearGeneration, refreshKey, enabled }: ImageGalleryProps) {
  const [operations, setOperations] = useState<ImageOperation[]>([]);
  const [error, setError] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const request = useMemo(() => ({ agentId, sessionId, clearGeneration, refreshKey, enabled, attempt }), [agentId, sessionId, clearGeneration, refreshKey, enabled, attempt]);
  useEffect(() => {
    if (!request.enabled) return;
    let active = true;
    const controller = new AbortController();
    let timer: number | undefined;
    const load = async () => {
      try {
        const page = await daemon.images.list(request.agentId, request.sessionId, { signal: controller.signal });
        if (!active) return;
        if (page.clearGeneration !== request.clearGeneration) {
          setOperations([]);
          setError(true);
          return;
        }
        setOperations(page.operations);
        setError(false);
        if (page.operations.some((operation) => operation.status === 'dispatching')) timer = window.setTimeout(() => void load(), 2000);
      } catch {
        if (active) { setOperations([]); setError(true); }
      }
    };
    void load();
    return () => { active = false; controller.abort(); window.clearTimeout(timer); };
  }, [request]);
  if (!enabled || (!operations.length && !error)) return null;
  const versions = operations.flatMap((operation) => operation.version ? [operation.version] : []);
  return <section aria-label="Chat images" className="mx-4 my-2 max-h-56 overflow-auto rounded-xl border border-line bg-panel p-3 text-sm">
    <div className="flex items-center justify-between gap-3"><h2 className="font-semibold">Images</h2><button type="button" className="studio-tool-button" onClick={() => setAttempt((value) => value + 1)}>Refresh images</button></div>
    {error && <p role="status">Image versions could not be loaded. Refresh images to try again.</p>}
    {versions.length > 0 && <ol className="mt-2 flex max-h-32 gap-2 overflow-auto" aria-label="Saved image versions">
      {versions.map((version, index) => <li key={version.id} className="min-w-32 max-w-56 rounded-lg border border-line p-2">
        <ImageArtifactLink versions={versions} versionId={version.id} label={`Open image version ${index + 1}`}>
          <span className="block text-xs text-ink-3">Version {index + 1}{version.parentAttachmentId ? ' · Edited' : ''}</span>
          <span className="block break-words">{revealInvisible(version.attachment.name).text}</span>
        </ImageArtifactLink>
      </li>)}
    </ol>}
    {operations.filter((operation) => operation.status !== 'succeeded').map((operation) => <p key={operation.id} role="status" className="mt-2">
      {operation.status === 'dispatching' ? 'Creating image…' : operation.status === 'outcome_unknown' ? 'The image request outcome is unknown. The provider may charge; it will not be retried automatically.' : 'The image request failed. Cost is unknown; the provider may charge.'}
    </p>)}
  </section>;
}
