import type { ControlMemoryPublication, MemoryPublication, MemoryPublicationsView } from '@animaos/sdk';
import { useEffect, useRef, useState } from 'react';
import { daemon } from '../../lib/daemon-api';

export type GardenPublicationsProps = {
  agentId: string;
  disabled?: boolean;
  load?: (agentId: string, options: { signal?: AbortSignal }) => Promise<MemoryPublicationsView>;
  control?: (agentId: string, id: string, input: ControlMemoryPublication) => Promise<MemoryPublication>;
};

export function GardenPublicationsPanel(props: GardenPublicationsProps) {
  return <Publications key={props.agentId} {...props} />;
}

const labels: Record<MemoryPublication['state'], string> = {
  queued: 'Queued locally', sending: 'Sending', unknown: 'Outcome unconfirmed',
  submitted_for_review: 'Awaiting Mother review', saved: 'Saved in Mother',
  conflict: 'Needs review', blocked: 'Sharing blocked', dismissed: 'Dismissed by Mother owner', cancelled: 'Cancelled locally',
};

function reasonText(reason: string): string {
  if (reason.startsWith('remote_request_rejected:')) {
    return 'Mother rejected this selection. It will not be retried. Review the current memory and create a corrected selection.';
  }
  const reasons: Record<string, string> = {
    approval_required: 'Anima permission is required for this Mother tool. Review its permissions, then resume.',
    binding_changed: 'The saved garden connection changed. This share remains tied to its original connection.',
    generation_changed: 'The garden was restored. This share cannot be sent to a different generation.',
    source_changed: 'The selected local memory changed. Review it before creating another share.',
    source_deleted: 'The selected local memory was removed.',
    permission_denied: 'Current tool permissions prevent sharing.',
    approval_unreadable: 'The current Mother grant cannot read the review outcome. No saved result is confirmed.',
    persistent_storage_required: 'Persistent local storage is required for durable sharing.',
    publication_storage_unavailable: 'The local sharing queue could not be opened. Its saved work remains unconfirmed.',
    selected_source_changed: 'The selected local memory changed. Review it before creating another share.',
    selected_source_missing: 'The selected local memory is no longer available. An earlier send may still need reconciliation.',
    workspace_generation_or_access_changed: 'The garden identity or access changed. This share remains tied to its original garden.',
    publication_authority_changed: 'The connection or its reviewed tools changed. Review the original sharing permissions before resuming.',
    policy_denied: 'Current tool permissions prevent sharing.',
    proposal_conflict: 'The garden record changed while awaiting review. Resolve or dismiss the proposal in Mother, then check its outcome.',
    connection_manually_disconnected: 'This connection was disconnected. Reconnect it before resuming the share.',
  };
  return reasons[reason] ?? 'Sharing needs attention. Check the garden connection and current tool permissions.';
}

function Publications({ agentId, disabled = false, load = daemon.memoryPublications, control = daemon.controlMemoryPublication }: GardenPublicationsProps) {
  const [expanded, setExpanded] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const [view, setView] = useState<MemoryPublicationsView | null>(null);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [uncertain, setUncertain] = useState(false);
  const [error, setError] = useState('');
  const active = useRef<AbortController | null>(null);
  const mutating = useRef(false);
  useEffect(() => {
    if (!expanded) return;
    // A successful mutation or explicit refresh advances this read revision.
    void refresh;
    const controller = new AbortController();
    active.current = controller;
    void load(agentId, { signal: controller.signal }).then(next => {
      if (controller.signal.aborted) return;
      setView(next); setError(''); setUncertain(false);
    }, () => {
      if (!controller.signal.aborted) setError('Could not load selected shares. Refresh to check their current state.');
    }).finally(() => { if (!controller.signal.aborted) setLoading(false); });
    return () => controller.abort();
  }, [agentId, expanded, refresh, load]);

  async function change(row: MemoryPublication, action: ControlMemoryPublication['action']) {
    const signal = active.current?.signal;
    if (!signal || signal.aborted || disabled || loading || uncertain || mutating.current) return;
    mutating.current = true; setBusy(true); setError('');
    try {
      await control(agentId, row.id, { expectedRevision: row.revision, action });
      if (!signal.aborted) { setLoading(true); setRefresh(value => value + 1); }
    } catch {
      if (!signal.aborted) {
        setUncertain(true);
        setError('The change could not be confirmed. Refresh selected shares before trying again.');
      }
    } finally {
      mutating.current = false;
      if (!signal.aborted) setBusy(false);
    }
  }

  return <div className="workspace-context-publications">
    <button type="button" className="settings-item-action" aria-expanded={expanded} disabled={busy} onClick={() => { if (!expanded) setLoading(true); setExpanded(value => !value); }}>
      {expanded ? 'Hide selected shares' : 'Show selected shares'}
    </button>
    {expanded && <>
      <p>Only explicitly selected memories enter this queue. Queued and awaiting-review shares are not yet saved in Mother.</p>
      {error && <p role="alert">{error}</p>}
      {loading && <p role="status">Checking selected shares…</p>}
      {view && !view.available && <p>{reasonText(view.reason ?? 'persistent_storage_required')}</p>}
      {view?.available && <>
        {view.items.length === 0 && <p>No selected shares.</p>}
        {Object.keys(view.counts).length > 0 && <p aria-label="Selected share totals">
          {Object.entries(view.counts).map(([state, count]) => `${labels[state as MemoryPublication['state']] ?? state}: ${count}`).join(' · ')}
        </p>}
        <ul>
          {view.items.map(row => <li key={row.id}>
            <strong>{row.title}</strong><p>{labels[row.state]}</p>
            {row.reason && <p>{reasonText(row.reason)}</p>}
            {row.canonical && <p>Garden revision {row.canonical.revision}. {row.canonical.ownerEdited && <span>Owner edited the approved memory.</span>}</p>}
            {(row.state === 'blocked' || (row.state === 'conflict' && row.proposalId)) && <button type="button" className="settings-item-action" disabled={disabled || busy || loading || uncertain} aria-label={`Resume ${row.title}`} onClick={() => void change(row, 'resume')}>
              {row.state === 'conflict' ? 'Check review outcome' : 'Resume'}
            </button>}
            {row.attempts === 0 && (row.state === 'queued' || row.state === 'blocked') && <button type="button" className="settings-item-action" disabled={disabled || busy || loading || uncertain} aria-label={`Cancel ${row.title}`} onClick={() => void change(row, 'cancel')}>Cancel unsent share</button>}
          </li>)}
        </ul>
        {view.omitted > 0 && <p>{view.omitted} older shares are not shown.</p>}
      </>}
      <button type="button" className="settings-item-action" aria-label="Refresh selected shares" disabled={busy || loading} onClick={() => { setLoading(true); setRefresh(value => value + 1); }}>Refresh</button>
    </>}
  </div>;
}
