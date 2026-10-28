import { useEffect, useRef, useState } from 'react';
import type { ProjectSummary, Session } from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';
import './projects.css';

export function canMoveProjectChat(session: Session): boolean {
  return session.kind === 'chat' && !session.mainChat && !session.parentRunId
    && ['web', 'api', 'import'].includes(session.origin) && session.activeRuns === 0;
}

export function MoveChatDialog({ session, onClose, onMoved }: {
  session: Session; onClose: () => void; onMoved: (session: Session) => void;
}) {
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [selected, setSelected] = useState(session.projectId ?? '');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const panel = useRef<HTMLDivElement>(null);
  const busyRef = useRef(false);
  useEffect(() => {
    const control = new AbortController();
    const previousFocus = document.activeElement as HTMLElement | null;
    panel.current?.querySelector<HTMLElement>('select')?.focus();
    void daemon.projects.list(session.agentId, { signal: control.signal }).then((items) => {
      if (!control.signal.aborted) setProjects(items);
    }).catch((failure: unknown) => {
      if (!control.signal.aborted) setError(failure instanceof Error ? failure.message : String(failure));
    });
    const key = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !busyRef.current) { event.preventDefault(); onClose(); }
      if (event.key !== 'Tab') return;
      const controls = Array.from(panel.current?.querySelectorAll<HTMLElement>('button:not(:disabled), select:not(:disabled)') ?? []);
      if (!controls.length) { event.preventDefault(); panel.current?.focus(); return; }
      const index = controls.indexOf(document.activeElement as HTMLElement);
      if (event.shiftKey && index <= 0) { event.preventDefault(); controls.at(-1)?.focus(); }
      else if (!event.shiftKey && (index === -1 || index === controls.length - 1)) { event.preventDefault(); controls[0].focus(); }
    };
    document.addEventListener('keydown', key);
    return () => { control.abort(); document.removeEventListener('keydown', key); previousFocus?.focus(); };
  }, [session.agentId, onClose]);
  return <div className="project-move-backdrop"><div ref={panel} tabIndex={-1} className="project-move" role="dialog" aria-modal="true" aria-label="Move chat to project">
    <h2>Move “{session.title}”</h2>
    <p>The chat stays with its current agent. Existing pins from this chat become unavailable in its previous project.</p>
    <label>Project<select disabled={busy} value={selected} onChange={(event) => setSelected(event.target.value)}>
      <option value="">No project</option>{projects.map((project) => <option key={project.id} value={project.id}>{project.title}</option>)}
    </select></label>
    {error && <p role="alert">{error}</p>}
    <div className="project-row"><button disabled={busy || !canMoveProjectChat(session) || selected === (session.projectId ?? '')}
      onClick={() => { setBusy(true); busyRef.current = true; setError(null);
        void daemon.setSessionProject(session.agentId, session.id, { projectId: selected || null,
          expectedBindingRevision: session.projectBindingRevision ?? 0,
          ...(selected ? { expectedProjectRevision: projects.find((project) => project.id === selected)?.revision } : {}) })
          .then((updated) => { onMoved(updated); onClose(); })
          .catch((failure: unknown) => setError(failure instanceof Error ? failure.message : String(failure)))
          .finally(() => { setBusy(false); busyRef.current = false; });
      }}>Move chat</button><button disabled={busy} onClick={onClose}>Cancel</button></div>
  </div></div>;
}
