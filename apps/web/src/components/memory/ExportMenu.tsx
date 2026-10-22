import { useEffect, useRef, useState } from 'react';
import type { MemoryExportFormat } from '@animaos/sdk';

import { COMPANION_UNREACHABLE } from '../../lib/approvals';
import { daemon } from '../../lib/daemon-api';
import { DaemonHttpError } from '@animaos/sdk';

const CHOICES: readonly {
  format: MemoryExportFormat;
  all: boolean;
  label: string;
}[] = [
  { format: 'json', all: false, label: 'JSON, this agent' },
  { format: 'md', all: false, label: 'Markdown, this agent' },
  { format: 'json', all: true, label: 'JSON, all agents' },
  { format: 'md', all: true, label: 'Markdown, all agents' },
];

/** The download's name, as the daemon names it: the agent id kept to safe
 *  characters, and today's date. */
export function exportFilename(
  agentId: string | null,
  format: MemoryExportFormat,
  now: Date = new Date(),
): string {
  const agent =
    agentId === null
      ? 'all'
      : agentId.slice(0, 64).replace(/[^A-Za-z0-9._-]/g, '_') || 'agent';
  return `animaos-memory-${agent}-${now.toISOString().slice(0, 10)}.${format}`;
}

/** Export menu: JSON or Markdown, for this agent or all agents. The file is
 *  fetched through the SDK as a blob and saved from the browser. */
export function ExportMenu({ agentId }: { agentId: string }) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    const away = (event: MouseEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', away);
    return () => document.removeEventListener('mousedown', away);
  }, [open]);

  const choose = async (format: MemoryExportFormat, all: boolean) => {
    setOpen(false);
    trigger.current?.focus();
    setBusy(true);
    setError(null);
    let url: string | undefined;
    try {
      const blob = await daemon.exportMemories(
        all ? { format } : { format, agentId },
      );
      url = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = url;
      link.download = exportFilename(all ? null : agentId, format);
      document.body.appendChild(link);
      link.click();
      link.remove();
    } catch (cause) {
      setError(
        cause instanceof DaemonHttpError
          ? cause.message
          : COMPANION_UNREACHABLE,
      );
    } finally {
      setBusy(false);
      if (url) {
        const revoke = url;
        window.setTimeout(() => URL.revokeObjectURL(revoke), 1000);
      }
    }
  };

  return (
    <div
      className="memory-export"
      ref={root}
      onKeyDown={(event) => {
        if (event.key === 'Escape' && open) {
          event.stopPropagation();
          setOpen(false);
          trigger.current?.focus();
        }
      }}
    >
      <button
        type="button"
        ref={trigger}
        className="studio-tool-button"
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={busy}
        onClick={() => setOpen((value) => !value)}
      >
        {busy ? 'Exporting…' : 'Export'}
      </button>
      {open && (
        <div className="memory-export-menu" role="menu" aria-label="Export">
          {CHOICES.map((choice) => (
            <button
              key={choice.label}
              type="button"
              role="menuitem"
              onClick={() => void choose(choice.format, choice.all)}
            >
              {choice.label}
            </button>
          ))}
        </div>
      )}
      {error && (
        <p className="memory-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
