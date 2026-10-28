import { useEffect, useId, useRef } from 'react';
import { createPortal } from 'react-dom';
import type { ProjectKnowledgeCitation } from '@animaos/sdk';
import { useLatest } from '../../hooks/useLatest';
import './projects.css';

/** Plain text only: a cited passage remains untrusted source material. */
export function ProjectCitationDialog({ citation, opening, error, onClose }: {
  citation: ProjectKnowledgeCitation | null; opening: boolean; error?: string | null; onClose: () => void;
}) {
  const titleId = useId();
  const close = useRef<HTMLButtonElement>(null);
  const onCloseLatest = useLatest(onClose);
  useEffect(() => {
    const previous = document.activeElement;
    close.current?.focus();
    return () => { if (previous instanceof HTMLElement && previous.isConnected) previous.focus(); };
  }, []);
  return createPortal(<div className="project-citation-backdrop" onClick={(event) => { if (event.target === event.currentTarget) onCloseLatest.current(); }}>
    <section className="project-citation-dialog" role="dialog" aria-modal="true" aria-labelledby={titleId} onKeyDown={(event) => {
      if (event.key === 'Escape') { event.preventDefault(); onCloseLatest.current(); }
      if (event.key === 'Tab') {
        const items = Array.from(event.currentTarget.querySelectorAll<HTMLElement>('button, [tabindex="0"]'));
        const first = items[0]; const last = items.at(-1);
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
        else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
      }
    }}>
      <div className="project-row"><h2 id={titleId}>{citation?.title ?? (error ? 'Citation unavailable' : 'Opening citation')}</h2>
        <button type="button" ref={close} onClick={onClose}>Close citation</button></div>
      {error && <p role="alert">{error}</p>}
      {opening ? <p role="status">Checking current source and project access…</p> : citation && <>
        <p className="project-citation-locator">{citation.locator}</p>
        <pre tabIndex={0}>{citation.text}</pre>
        <p className="project-note">Bounded passage verified against the current source when opened.</p>
      </>}
    </section>
  </div>, document.body);
}
