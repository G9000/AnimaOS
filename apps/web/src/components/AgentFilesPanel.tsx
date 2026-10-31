import { useEffect, useState } from 'react';
import {
  DaemonHttpError,
  type AgentFileManifestEntry,
  type AgentFilesPreview,
} from '@animaos/sdk';
import { daemon } from '../lib/daemon-api';
import { useResetOnChange } from '../hooks/useResetOnChange';
import {
  SettingsCard,
  SettingsEmpty,
  SettingsRow,
  SettingsSection,
} from './settings/SettingsKit';

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

/** Where the personality comes from, in plain words. */
const IDENTITY_LABELS: Record<AgentFilesPreview['identitySource'], string> = {
  legacy: 'Saved in settings (no SOUL.md file)',
  workspace_soul: 'SOUL.md in the workspace',
  profile_soul: 'This agent’s own SOUL.md',
};

const STATE_LABELS: Record<AgentFileManifestEntry['state'], string> = {
  loaded: 'Used',
  truncated: 'Used, cut short',
  shadowed: 'Not used: a higher-priority source was selected',
  duplicate: 'Not used: same as another file',
  empty: 'Not used: empty',
  missing: 'Not found',
  unreadable: 'Not used: could not be read',
  invalid_utf8: 'Not used: not UTF-8 text',
  linked: 'Not used: a link, not a file',
  non_regular: 'Not used: not a regular file',
  oversized: 'Not used: too large',
  blocked: 'Blocked: suspicious instructions',
  budget_excluded: 'Not used: over the size budget',
  timed_out: 'Not used: took too long to read',
  cancelled: 'Not used: reading was cancelled',
};

const used = (entry: AgentFileManifestEntry) =>
  entry.state === 'loaded' || entry.state === 'truncated';

/**
 * Project rule files (AGENTS.md, CLAUDE.md, Cursor rules) the agent reads
 * from its workspace, as a current preview. They are edited in the folder;
 * new runs read the current files, while an active run keeps its accepted
 * snapshot, including after compaction.
 */
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

  useResetOnChange([agentId, attempt, loadPreview], () => {
    setState({ agentId, status: 'loading' });
  });
  useEffect(() => {
    const controller = new AbortController();
    let current = true;
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
  const refresh = () => setAttempt((value) => value + 1);
  return (
    <SettingsSection
      id="agent-files"
      className="agent-files"
      title="Project rules it reads"
      description="AGENTS.md, CLAUDE.md or Cursor rules in the workspace folder are added to its instructions. Edit them in the folder; new replies read the current files, while a reply in progress keeps its accepted snapshot."
      action={
        visible.status === 'ready' && (
          <button type="button" className="ui-button is-quiet" onClick={refresh}>
            Refresh
          </button>
        )
      }
    >
      {visible.status === 'loading' && (
        <SettingsCard>
          <p role="status" className="sk-empty">
            Loading project rules…
          </p>
        </SettingsCard>
      )}
      {visible.status === 'unavailable' && (
        <SettingsCard>
          <p role="status" className="sk-empty">
            The rules preview is unavailable for this agent.
          </p>
        </SettingsCard>
      )}
      {visible.status === 'error' && (
        <div role="alert" className="agent-files-error">
          <p>Could not load the rules preview.</p>
          <button type="button" className="ui-button" onClick={refresh}>
            Retry preview
          </button>
        </div>
      )}
      {visible.status === 'ready' && <PreviewDetails preview={visible.preview} />}
    </SettingsSection>
  );
}

function PreviewDetails({ preview }: { preview: AgentFilesPreview }) {
  if (preview.availability !== 'available') {
    return (
      <SettingsCard>
        <p role="status" className="sk-empty">
          {preview.availability === 'unavailable_workspace'
            ? 'Configure a workspace to preview project rules.'
            : 'Agent settings changed during the preview. Refresh to try again.'}
        </p>
      </SettingsCard>
    );
  }
  // Files it looked for but did not find stay in the technical details.
  const found = preview.entries.filter((entry) => entry.state !== 'missing');
  return (
    <>
      <SettingsCard>
        <SettingsRow
          title="Personality"
          control={
            <span className="sk-meta">
              {IDENTITY_LABELS[preview.identitySource]}
            </span>
          }
        />
        <SettingsRow
          title="Project rules"
          control={
            <span className="sk-meta">
              {preview.projectSourceKind
                ? PROJECT_SOURCE_LABELS[preview.projectSourceKind]
                : 'None found'}
            </span>
          }
        />
        <SettingsRow
          title="Added to every reply"
          control={
            <span className="sk-meta">
              About {preview.estimatedTokens.toLocaleString()} tokens
            </span>
          }
        />
      </SettingsCard>
      <SettingsCard as="ul" label="Files found">
        {found.length === 0 ? (
          <li>
            <SettingsEmpty>No rule files found in the workspace.</SettingsEmpty>
          </li>
        ) : (
          found.map((entry, index) => (
            <SettingsRow
              as="li"
              key={`${entry.relativePath}-${index}`}
              className="agent-file"
              title={<code className="agent-file-path">{entry.relativePath}</code>}
              description={
                used(entry)
                  ? `${entry.admittedCharCount.toLocaleString()} characters`
                  : undefined
              }
              control={
                <span
                  className={`sk-badge${entry.state === 'blocked' ? ' is-danger' : used(entry) ? ' is-ok' : ''}`}
                >
                  {STATE_LABELS[entry.state] ?? entry.state}
                </span>
              }
            />
          ))
        )}
      </SettingsCard>
      <details className="sk-disclosure agent-files-technical">
        <summary>Technical details</summary>
        <div className="agent-files-technical-body">
          <p>
            Current preview · Captured{' '}
            {new Date(preview.capturedAtMs).toLocaleString()}
          </p>
          <p className="agent-files-mono">
            Snapshot digest: {preview.snapshotDigest}
          </p>
          {preview.entries.map((entry, index) => (
            <div key={`${entry.relativePath}-${index}`}>
              <p className="agent-files-mono">{entry.relativePath}</p>
              <p>
                {entry.state} · {entry.rawByteCount} bytes ·{' '}
                {entry.admittedCharCount} admitted characters
                {entry.truncated ? ' · truncated' : ''}
              </p>
              {entry.digest && (
                <p className="agent-files-mono">Digest: {entry.digest}</p>
              )}
              {entry.reasonCode && <p>Reason: {entry.reasonCode}</p>}
              {entry.threatIds.length > 0 && (
                <p>Threat IDs: {entry.threatIds.join(', ')}</p>
              )}
            </div>
          ))}
        </div>
      </details>
    </>
  );
}
