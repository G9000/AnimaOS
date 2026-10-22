import { useEffect, useState } from 'react';
import { DaemonHttpError, type AgentFilesPreview } from '@animaos/sdk';
import { daemon } from '../lib/daemon-api';
import { SectionTitle } from './ui-bits';

type PreviewLoader = (
  id: string,
  options: { signal?: AbortSignal },
) => Promise<AgentFilesPreview>;

type PreviewState =
  | { agentId: string; status: 'loading' | 'unavailable' | 'error' }
  | { agentId: string; status: 'ready'; preview: AgentFilesPreview };

const PROJECT_SOURCE_LABELS = {
  anima: 'Anima',
  agents: 'AGENTS.md',
  claude: 'CLAUDE.md',
  cursor: 'Cursor rules',
};

export function AgentFilesPanel({
  agentId,
  loadPreview = daemon.agentFilesPreview,
}: {
  agentId: string;
  loadPreview?: PreviewLoader;
}) {
  const [attempt, setAttempt] = useState(0);
  const [state, setState] = useState<PreviewState>({
    agentId,
    status: 'loading',
  });

  useEffect(() => {
    const controller = new AbortController();
    let current = true;
    setState({ agentId, status: 'loading' });
    void loadPreview(agentId, { signal: controller.signal }).then(
      (preview) => {
        if (current) setState({ agentId, status: 'ready', preview });
      },
      (error: unknown) => {
        if (!current) return;
        setState({
          agentId,
          status:
            error instanceof DaemonHttpError && error.status === 404
              ? 'unavailable'
              : 'error',
        });
      },
    );
    return () => {
      current = false;
      controller.abort();
    };
  }, [agentId, attempt, loadPreview]);

  const visible =
    state.agentId === agentId ? state : { status: 'loading' as const };
  return (
    <section className="space-y-2.5" aria-label="Agent files">
      <SectionTitle>Agent files</SectionTitle>
      <p className="text-sm leading-relaxed text-ink-3">
        Current preview of instruction files. New runs read current files; an
        active run keeps its accepted snapshot, including after compaction.
      </p>
      {visible.status === 'loading' && (
        <p role="status">Loading agent files…</p>
      )}
      {visible.status === 'unavailable' && (
        <p role="status">Agent file preview is unavailable for this agent.</p>
      )}
      {visible.status === 'error' && (
        <div role="alert">
          <p>Could not load the agent file preview.</p>
          <button
            className="text-sm underline"
            onClick={() => setAttempt((value) => value + 1)}
          >
            Retry preview
          </button>
        </div>
      )}
      {visible.status === 'ready' && (
        <>
          <PreviewDetails preview={visible.preview} />
          <button
            className="text-sm underline"
            onClick={() => setAttempt((value) => value + 1)}
          >
            Refresh preview
          </button>
        </>
      )}
    </section>
  );
}

function PreviewDetails({ preview }: { preview: AgentFilesPreview }) {
  if (preview.availability !== 'available') {
    return (
      <p role="status">
        {preview.availability === 'unavailable_workspace'
          ? 'Configure a workspace to preview agent files.'
          : 'Agent settings changed during the preview. Refresh to try again.'}
      </p>
    );
  }
  return (
    <div className="space-y-3 text-xs text-ink-3">
      <p>
        Current preview · Captured{' '}
        {new Date(preview.capturedAtMs).toLocaleString()}
      </p>
      <p>
        Identity:{' '}
        {
          {
            legacy: 'Legacy identity',
            workspace_soul: 'Workspace SOUL.md',
            profile_soul: 'Agent profile SOUL.md',
          }[preview.identitySource]
        }{' '}
        · Project instructions:{' '}
        {preview.projectSourceKind
          ? PROJECT_SOURCE_LABELS[preview.projectSourceKind]
          : 'none'}
      </p>
      <p>{preview.estimatedTokens} estimated tokens</p>
      <p className="break-all font-mono">
        Snapshot digest: {preview.snapshotDigest}
      </p>
      {preview.entries.length === 0 && (
        <p>No instruction files found. Legacy identity remains in use.</p>
      )}
      <ul className="space-y-2">
        {preview.entries.map((entry, index) => (
          <li
            key={`${entry.relativePath}-${index}`}
            className="rounded-lg border border-line p-3"
          >
            <p className="break-all font-mono">{entry.relativePath}</p>
            <p>
              {entry.state}
              {entry.state === 'shadowed'
                ? ' · a higher-priority source was selected'
                : ''}
            </p>
            <p>
              {entry.rawByteCount} bytes · {entry.admittedCharCount} admitted
              characters{entry.truncated ? ' · truncated' : ''}
            </p>
            {entry.digest && (
              <p className="break-all font-mono">Digest: {entry.digest}</p>
            )}
            {entry.reasonCode && <p>Reason: {entry.reasonCode}</p>}
            {entry.threatIds.length > 0 && (
              <p>Threat IDs: {entry.threatIds.join(', ')}</p>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
