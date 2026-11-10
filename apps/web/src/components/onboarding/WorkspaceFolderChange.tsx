import { useEffect, useRef, useState } from 'react';
import { DaemonHttpError } from '@animaos/sdk';
import {
  daemon,
  type DaemonWorkspaceState,
  type WorkspaceConfigInput,
} from '../../lib/daemon-api';

/** Changes the daemon's shared project root; it never imports or resets agents. */
export function WorkspaceFolderChange({
  workspace,
  disabled,
  onEditingChange,
  onChanged,
}: {
  workspace: DaemonWorkspaceState;
  disabled: boolean;
  onEditingChange(editing: boolean): void;
  onChanged(workspace: DaemonWorkspaceState): void;
}) {
  const [editing, setEditing] = useState(false);
  const [path, setPath] = useState('');
  const [checked, setChecked] = useState<WorkspaceConfigInput | null>(null);
  const [busy, setBusy] = useState(false);
  const [uncertain, setUncertain] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mounted = useRef(true);
  const pending = useRef(false);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const finish = (state: DaemonWorkspaceState) => {
    onChanged(state);
    setEditing(false);
    setUncertain(false);
    setChecked(null);
    onEditingChange(false);
  };
  const reconcile = async () => {
    const state = await daemon.getWorkspace();
    if (!mounted.current) return;
    let expectedRoot = checked?.rootPath;
    if (checked && state.workspace?.rootPath !== expectedRoot) {
      // A missing folder is created and canonicalized by PUT, so validation
      // may now return a different spelling (notably Windows' extended paths).
      const validated = await daemon.validateWorkspace(checked);
      if (!mounted.current) return;
      expectedRoot = validated.workspace?.rootPath;
    }
    if (expectedRoot && state.workspace?.rootPath === expectedRoot)
      finish(state);
    else {
      setError(
        `The server currently reports ${state.workspace?.rootPath ?? 'no home folder'}. The earlier save is still unconfirmed. Check again; it has not been repeated.`,
      );
    }
  };
  const run = async (action: () => Promise<void>) => {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (caught) {
      if (mounted.current)
        setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      pending.current = false;
      if (mounted.current) setBusy(false);
    }
  };
  const check = () =>
    run(async () => {
      setChecked(null);
      const current = workspace.workspace;
      if (!current)
        throw new Error('Reload the workspace before changing its folder.');
      const result = await daemon.validateWorkspace({
        rootPath: path.trim(),
        companyName: current.companyName,
        mission: current.mission,
        values: current.values,
      });
      if (mounted.current && result.workspace) {
        setChecked({
          rootPath: result.workspace.rootPath,
          companyName: current.companyName,
          mission: current.mission,
          values: current.values,
        });
      }
    });
  const save = () =>
    run(async () => {
      if (!checked) return;
      const fresh = await daemon.getWorkspace();
      if (!mounted.current) return;
      if (
        !fresh.workspace ||
        fresh.workspace.rootPath !== workspace.workspace?.rootPath
      ) {
        finish(fresh);
        throw new Error(
          'The workspace changed elsewhere. Review its current folder before trying again.',
        );
      }
      const input = {
        rootPath: checked.rootPath,
        companyName: fresh.workspace.companyName,
        mission: fresh.workspace.mission,
        values: fresh.workspace.values,
      };
      // Once sent, a transport error is uncertain. Only a read may reconcile it.
      setUncertain(true);
      try {
        // PUT returns the persisted workspace. Its successful response is
        // authoritative; another GET must not turn completion into uncertainty.
        const saved = await daemon.putWorkspace(input);
        if (!mounted.current) return;
        finish(saved);
        if (saved.workspace?.rootPath !== input.rootPath)
          setError(
            'The server returned a different home folder. Review its current path before continuing.',
          );
      } catch (caught) {
        if (
          caught instanceof DaemonHttpError &&
          [400, 401, 403].includes(caught.status)
        ) {
          if (mounted.current) {
            setUncertain(false);
            setChecked(null);
            setError(caught.message);
          }
          return;
        }
        if (mounted.current)
          setError(
            'Could not confirm the folder change. Check the saved folder before continuing.',
          );
      }
    });
  if (!editing)
    return (
      <>
        <button
          type="button"
          className="onboarding__link"
          disabled={disabled}
          onClick={() => {
            setPath(workspace.workspace?.rootPath ?? '');
            setError(null);
            setChecked(null);
            setEditing(true);
            onEditingChange(true);
          }}
        >
          Change folder
        </button>
        {error && (
          <p role="alert" className="onboarding__hint">
            {error}
          </p>
        )}
      </>
    );
  return (
    <div className="onboarding__section">
      <p className="onboarding__hint">
        This changes the home folder for all agents on this server. Saved
        agents, chats and M.O.T.H.E.R memory are kept. Existing files stay in
        their current folders; this does not import another workspace.
      </p>
      <label htmlFor="new-workspace-folder" className="onboarding__label">
        New home folder
      </label>
      <input
        id="new-workspace-folder"
        className="onboarding__input onboarding__input--mono"
        value={path}
        disabled={busy || uncertain || disabled}
        onChange={(e) => {
          setPath(e.target.value);
          setChecked(null);
          setError(null);
        }}
      />
      {checked && (
        <p className="onboarding__hint">
          Use {checked.rootPath} as the shared home folder?
        </p>
      )}
      <div className="onboarding__row">
        {!uncertain && (
          <button
            type="button"
            className="onboarding__button onboarding__button--secondary"
            disabled={busy || disabled}
            onClick={() =>
              void run(async () => {
                const selected = await daemon.pickWorkspaceFolder();
                if (mounted.current && selected.rootPath) {
                  setPath(selected.rootPath);
                  setChecked(null);
                }
              })
            }
          >
            Browse
          </button>
        )}
        {uncertain ? (
          <button
            type="button"
            className="onboarding__button onboarding__button--secondary"
            disabled={busy || disabled}
            onClick={() => void run(() => reconcile())}
          >
            Check saved folder
          </button>
        ) : (
          <button
            type="button"
            className="onboarding__button onboarding__button--secondary"
            disabled={busy || disabled || !path.trim()}
            onClick={() => void (checked ? save() : check())}
          >
            {busy ? 'Checking…' : checked ? 'Use this folder' : 'Check folder'}
          </button>
        )}
        {!uncertain && (
          <button
            type="button"
            className="onboarding__link"
            disabled={busy || disabled}
            onClick={() => {
              setEditing(false);
              onEditingChange(false);
            }}
          >
            Cancel folder change
          </button>
        )}
      </div>
      {error && (
        <p role="alert" className="onboarding__hint">
          {error}
        </p>
      )}
    </div>
  );
}
