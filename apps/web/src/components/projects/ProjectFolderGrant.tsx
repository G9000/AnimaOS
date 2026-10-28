import { useEffect, useRef, useState } from 'react';
import { daemon } from '../../lib/daemon-api';

/** Choosing a folder is a preview; only the checked submission grants a read capability. */
export function ProjectFolderGrant({ onAdd, onClose, canGrant }: {
  onAdd: (rootPath: string, label: string, requestKey: string) => Promise<void>; onClose: () => void; canGrant: boolean;
}) {
  const [rootPath, setRootPath] = useState('');
  const [label, setLabel] = useState('');
  const [consent, setConsent] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const key = useRef(crypto.randomUUID());
  const alive = useRef(true);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  const choose = async () => {
    setBusy(true); setError(null);
    try {
      const result = await daemon.pickKnowledgeFolder();
      if (!alive.current || !result.rootPath) return;
      setRootPath(result.rootPath); setLabel(result.rootPath.replace(/[\\/]+$/, '').split(/[\\/]/).at(-1) ?? '');
      setConsent(false); key.current = crypto.randomUUID();
    } catch (failure) { if (alive.current) setError(failure instanceof Error ? failure.message : String(failure)); }
    finally { if (alive.current) setBusy(false); }
  };
  return <form className="project-picker" aria-label="Add knowledge folder" onSubmit={(event) => {
    event.preventDefault(); if (!canGrant || !consent || !rootPath || busy) return; setBusy(true); setError(null);
    void onAdd(rootPath, label.trim(), key.current)
      .catch((failure: unknown) => { if (alive.current) setError(failure instanceof Error ? failure.message : String(failure)); })
      .finally(() => { if (alive.current) setBusy(false); });
  }}>
    <h3>Add a folder to project knowledge</h3>
    <button type="button" disabled={busy} onClick={() => void choose()}>Choose folder</button>
    {rootPath && <><p className="project-folder-path">{rootPath}</p>
      <label>Folder label<input maxLength={120} value={label} disabled={busy} onChange={(event) => { setLabel(event.target.value); key.current = crypto.randomUUID(); }} /></label>
      <p>This folder and its supported documents will be readable by this project’s current member agents for search and bounded passages in project chats.</p>
      <label className="project-check"><input type="checkbox" checked={consent} disabled={busy} onChange={(event) => setConsent(event.target.checked)} />
        I grant this project read-only access to this folder.</label>
      <p className="project-note">The folder stays in its current location. Agents keep their existing workspace, tools, private memory, and permissions. Unlink the source to revoke this project’s access.</p></>}
    {error && <p role="alert">{error}</p>}
    {!canGrant && <p role="status">Waiting for current project access before granting this folder.</p>}
    <div className="project-row"><button disabled={!canGrant || busy || !rootPath || !consent}>Grant read-only access and add folder</button>
      <button type="button" onClick={onClose}>Cancel</button></div>
  </form>;
}
