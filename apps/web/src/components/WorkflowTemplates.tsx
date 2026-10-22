import { useEffect, useRef, useState } from 'react';
import {
  DaemonHttpError,
  type WorkflowRun,
  type WorkflowStepInput,
  type WorkflowTemplate,
  type WorkflowTemplateInput,
  type WorkflowTemplateRunInput,
} from '@animaos/sdk';
import { daemon } from '../lib/daemon-api';

const button =
  'rounded-xl border border-line px-4 py-3 text-sm disabled:opacity-50';
const bytes = (text: string) => new TextEncoder().encode(text).byteLength;
const step = (): WorkflowStepInput => ({
  id: crypto.randomUUID(),
  title: '',
  kind: 'task',
  prompt: '',
});
const fresh = (): WorkflowTemplateInput => ({
  slug: '',
  revision: 0,
  title: '',
  parameters: [],
  steps: [step()],
});
type Command =
  | { type: 'save'; input: WorkflowTemplateInput }
  | { type: 'run'; slug: string; input: WorkflowTemplateRunInput }
  | { type: 'delete'; slug: string; input: { revision: number } };

function restore(key: string): Command | null {
  try {
    const value: unknown = JSON.parse(sessionStorage.getItem(key) ?? 'null');
    if (
      !value ||
      typeof value !== 'object' ||
      !('type' in value) ||
      !['save', 'run', 'delete'].includes(String(value.type)) ||
      !('input' in value) ||
      typeof value.input !== 'object' ||
      !value.input
    )
      return null;
    // The daemon performs strict validation; restored text never becomes executable markup.
    return value as Command;
  } catch {
    return null;
  }
}
function remember(key: string, command: Command | null) {
  try {
    if (command) sessionStorage.setItem(key, JSON.stringify(command));
    else sessionStorage.removeItem(key);
  } catch {
    /* In-memory retry remains available. */
  }
}
function invalid(input: WorkflowTemplateInput): boolean {
  return (
    !/^[a-z0-9_-]{1,64}$/.test(input.slug) ||
    !input.title.trim() ||
    Array.from(input.title).length > 160 ||
    input.steps.length < 1 ||
    input.steps.length > 16 ||
    input.steps.some(
      (item) =>
        !item.title.trim() ||
        Array.from(item.title).length > 160 ||
        !item.prompt.trim() ||
        bytes(item.prompt) > 8192,
    ) ||
    input.parameters.length > 16 ||
    input.parameters.some(
      (item) =>
        !/^[A-Za-z_][A-Za-z0-9_]{0,63}$/.test(item.name) ||
        bytes(item.default ?? '') > 8192,
    ) ||
    new Set(input.parameters.map((item) => item.name)).size !==
      input.parameters.length ||
    bytes(JSON.stringify(input)) > 64 * 1024
  );
}

/** Owner controls; every run creates an immutable workflow with its own checkpoints. */
export function WorkflowTemplates({
  agentId,
  online,
  workflows = [],
  onRun,
  refreshKey,
}: {
  agentId: string;
  online: boolean;
  workflows?: readonly WorkflowRun[];
  onRun?: () => void;
  refreshKey?: string | number;
}) {
  const commandKey = `anima:template-command:${agentId}`;
  const [templates, setTemplates] = useState<WorkflowTemplate[] | null>(null);
  const [selection, setSelection] = useState('');
  const [inputs, setInputs] = useState<Record<string, string>>({});
  const [draft, setDraft] = useState<WorkflowTemplateInput | null>(null);
  const [pending, setPending] = useState<Command | null>(() =>
    restore(commandKey),
  );
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const epoch = useRef(0);
  const busyRef = useRef(false);
  const selected = templates?.find((item) => item.slug === selection);
  useEffect(() => {
    const current = ++epoch.current;
    const controller = new AbortController();
    setTemplates(null);
    if (online) {
      daemon
        .listWorkflowTemplates(agentId, { signal: controller.signal })
        .then((records) => {
          if (current === epoch.current) setTemplates(records);
        })
        .catch((reason: unknown) => {
          if (current === epoch.current)
            setError(reason instanceof Error ? reason.message : String(reason));
        });
    }
    return () => {
      epoch.current++;
      controller.abort();
    };
  }, [agentId, online, refresh, refreshKey]);

  const submit = async (command: Command) => {
    if (!online || !templates || busyRef.current) return;
    const current = epoch.current;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    setPending(command);
    remember(commandKey, command);
    try {
      if (command.type === 'save')
        await daemon.saveWorkflowTemplate(agentId, command.input);
      else if (command.type === 'delete')
        await daemon.deleteWorkflowTemplate(
          agentId,
          command.slug,
          command.input,
        );
      else
        await daemon.runWorkflowTemplate(agentId, command.slug, command.input);
      // Clear the originating agent's durable retry receipt even if the view changed.
      remember(commandKey, null);
      if (command.type === 'run') onRun?.();
      if (current !== epoch.current) return;
      setPending(null);
      if (command.type === 'save' || command.type === 'delete') setDraft(null);
      setRefresh((value) => value + 1);
    } catch (reason) {
      if (current !== epoch.current) return;
      setError(reason instanceof Error ? reason.message : String(reason));
      if (
        reason instanceof DaemonHttpError &&
        reason.status >= 400 &&
        reason.status < 500
      ) {
        setPending(null);
        remember(commandKey, null);
        setRefresh((value) => value + 1);
      }
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };
  const disabled = !online || templates === null || busy || pending !== null;
  const edit = (patch: Partial<WorkflowTemplateInput>) =>
    setDraft((previous) => (previous ? { ...previous, ...patch } : previous));
  const editStep = (index: number, patch: Partial<WorkflowStepInput>) =>
    draft &&
    edit({
      steps: draft.steps.map((item, position) =>
        position === index ? { ...item, ...patch } : item,
      ),
    });
  const choose = (slug: string) => {
    setSelection(slug);
    setInputs({});
  };
  const requiredMissing = selected?.parameters.some(
    (parameter) => parameter.default == null && !inputs[parameter.name]?.trim(),
  );
  const valuesTooLarge =
    Object.values(inputs).some((value) => bytes(value) > 8192) ||
    Object.values(inputs).reduce((total, value) => total + bytes(value), 0) >
      16384;
  return (
    <section
      aria-label="Reusable workflow templates"
      className="space-y-4 rounded-2xl border border-line p-5"
    >
      <h3 className="text-base font-semibold">Reusable workflow templates</h3>
      <p className="text-sm text-ink-3">
        Save ordered steps once. Use {'${name}'} for named inputs and{' '}
        {'$${name}'} for literal text. Each run keeps its own inputs, template
        revision, results, and owner checkpoints.
      </p>
      {error && (
        <p role="alert" className="text-sm text-danger">
          {error}
        </p>
      )}
      {!online && <p role="status">Offline. Reconnect to use templates.</p>}
      {online && templates === null && <p role="status">Loading templates…</p>}
      {pending && (
        <div className="space-y-2 rounded-xl border border-line p-3">
          <p className="text-sm">
            The response was not confirmed. Retry the same saved template
            request before starting another.
          </p>
          <pre className="max-h-32 overflow-auto whitespace-pre-wrap break-words text-xs">
            {JSON.stringify(pending.input, null, 2)}
          </pre>
          <button
            type="button"
            className={button}
            disabled={!online || templates === null || busy}
            onClick={() => void submit(pending)}
          >
            Retry exact template request
          </button>
        </div>
      )}
      <label className="block space-y-2 text-sm">
        Template to run
        <select
          className="field"
          value={selection}
          disabled={disabled}
          onChange={(event) => choose(event.target.value)}
        >
          <option value="">Choose a template</option>
          {templates?.map((template) => (
            <option key={template.slug} value={template.slug}>
              {template.title} · revision {template.revision}
            </option>
          ))}
        </select>
      </label>
      {selected && (
        <div className="space-y-3">
          <p className="text-xs text-ink-3">
            {selected.slug} · revision {selected.revision} ·{' '}
            {selected.steps.length} steps
          </p>
          {selected.parameters.map((parameter) => (
            <label key={parameter.name} className="block space-y-2 text-sm">
              Input {parameter.name}
              <input
                className="field"
                value={inputs[parameter.name] ?? ''}
                disabled={disabled}
                placeholder={parameter.default ?? 'Required'}
                onChange={(event) =>
                  setInputs((previous) => ({
                    ...previous,
                    [parameter.name]: event.target.value,
                  }))
                }
              />
              {parameter.default != null && (
                <span className="block text-xs text-ink-3">
                  Default: {parameter.default}
                </span>
              )}
            </label>
          ))}
          <details>
            <summary className="cursor-pointer text-sm">
              Review template steps
            </summary>
            {selected.steps.map((item) => (
              <div key={item.id} className="space-y-1 py-2 text-sm">
                <p>
                  {item.title} · {item.kind}
                </p>
                <pre className="whitespace-pre-wrap break-words">
                  {item.prompt}
                </pre>
              </div>
            ))}
          </details>
        </div>
      )}
      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          className={button}
          disabled={disabled || !selected || requiredMissing || valuesTooLarge}
          onClick={() =>
            selected &&
            void submit({
              type: 'run',
              slug: selected.slug,
              input: {
                revision: selected.revision,
                requestKey: crypto.randomUUID(),
                inputs,
              },
            })
          }
        >
          Run template
        </button>
        <button
          type="button"
          className={button}
          disabled={disabled}
          onClick={() => setDraft(fresh())}
        >
          New template
        </button>
        {selected && (
          <button
            type="button"
            className={button}
            disabled={disabled}
            onClick={() =>
              setDraft({
                slug: selected.slug,
                revision: selected.revision,
                title: selected.title,
                parameters: selected.parameters.map((item) => ({ ...item })),
                steps: selected.steps.map((item) => ({ ...item })),
              })
            }
          >
            Edit template
          </button>
        )}
      </div>
      {draft && (
        <form
          className="space-y-3 border-t border-line pt-4"
          onSubmit={(event) => {
            event.preventDefault();
            if (!invalid(draft)) void submit({ type: 'save', input: draft });
          }}
        >
          <h4 className="font-semibold">
            {draft.revision
              ? `Edit revision ${draft.revision}`
              : 'New reusable template'}
          </h4>
          <fieldset className="space-y-3" disabled={disabled}>
            {workflows.length > 0 && (
              <label className="block space-y-2 text-sm">
                Copy steps from workflow
                <select
                  className="field"
                  value=""
                  onChange={(event) => {
                    const source = workflows.find(
                      (item) => item.id === event.target.value,
                    );
                    if (source)
                      edit({
                        title: source.title,
                        steps: source.steps.map(
                          ({ id, title, kind, prompt }) => ({
                            id,
                            title,
                            kind,
                            prompt,
                          }),
                        ),
                      });
                  }}
                >
                  <option value="">Choose saved workflow</option>
                  {workflows.map((workflow) => (
                    <option key={workflow.id} value={workflow.id}>
                      {workflow.title}
                    </option>
                  ))}
                </select>
              </label>
            )}
            <label className="block space-y-2 text-sm">
              Template slug
              <input
                className="field"
                value={draft.slug}
                disabled={draft.revision > 0}
                maxLength={64}
                onChange={(event) => edit({ slug: event.target.value })}
              />
            </label>
            <label className="block space-y-2 text-sm">
              Template title
              <input
                className="field"
                value={draft.title}
                maxLength={320}
                onChange={(event) => edit({ title: event.target.value })}
              />
            </label>
            {draft.parameters.map((parameter, index) => (
              <div
                key={index}
                className="space-y-2 rounded-xl border border-line p-3"
              >
                <label className="block space-y-2 text-sm">
                  Parameter {index + 1} name
                  <input
                    className="field"
                    value={parameter.name}
                    maxLength={64}
                    onChange={(event) =>
                      edit({
                        parameters: draft.parameters.map((item, position) =>
                          position === index
                            ? { ...item, name: event.target.value }
                            : item,
                        ),
                      })
                    }
                  />
                </label>
                <label className="flex gap-2 text-sm">
                  <input
                    type="checkbox"
                    checked={parameter.default != null}
                    onChange={(event) =>
                      edit({
                        parameters: draft.parameters.map((item, position) =>
                          position === index
                            ? {
                                ...item,
                                default: event.target.checked ? '' : null,
                              }
                            : item,
                        ),
                      })
                    }
                  />
                  Parameter {index + 1} has default
                </label>
                {parameter.default != null && (
                  <label className="block space-y-2 text-sm">
                    Parameter {index + 1} default
                    <input
                      className="field"
                      value={parameter.default}
                      onChange={(event) =>
                        edit({
                          parameters: draft.parameters.map((item, position) =>
                            position === index
                              ? { ...item, default: event.target.value }
                              : item,
                          ),
                        })
                      }
                    />
                  </label>
                )}
                <button
                  type="button"
                  className={button}
                  onClick={() =>
                    edit({
                      parameters: draft.parameters.filter(
                        (_, position) => position !== index,
                      ),
                    })
                  }
                >
                  Remove parameter {index + 1}
                </button>
              </div>
            ))}
            <button
              type="button"
              className={button}
              disabled={draft.parameters.length >= 16}
              onClick={() =>
                edit({
                  parameters: [
                    ...draft.parameters,
                    { name: '', default: null },
                  ],
                })
              }
            >
              Add parameter
            </button>
            {draft.steps.map((item, index) => (
              <div
                key={item.id}
                className="space-y-2 rounded-xl border border-line p-3"
              >
                <label className="block space-y-2 text-sm">
                  Template step {index + 1} kind
                  <select
                    className="field"
                    value={item.kind}
                    onChange={(event) =>
                      editStep(index, {
                        kind: event.target.value as WorkflowStepInput['kind'],
                      })
                    }
                  >
                    <option value="task">Task</option>
                    <option value="approval">Approval checkpoint</option>
                    <option value="input">Question</option>
                  </select>
                </label>
                <label className="block space-y-2 text-sm">
                  Template step {index + 1} title
                  <input
                    className="field"
                    value={item.title}
                    onChange={(event) =>
                      editStep(index, { title: event.target.value })
                    }
                  />
                </label>
                <label className="block space-y-2 text-sm">
                  Template step {index + 1} prompt
                  <textarea
                    className="field min-h-24"
                    value={item.prompt}
                    onChange={(event) =>
                      editStep(index, { prompt: event.target.value })
                    }
                  />
                </label>
                <button
                  type="button"
                  className={button}
                  disabled={draft.steps.length === 1}
                  onClick={() =>
                    edit({
                      steps: draft.steps.filter(
                        (_, position) => position !== index,
                      ),
                    })
                  }
                >
                  Remove template step {index + 1}
                </button>
              </div>
            ))}
            <button
              type="button"
              className={button}
              disabled={draft.steps.length >= 16}
              onClick={() => edit({ steps: [...draft.steps, step()] })}
            >
              Add template step
            </button>
          </fieldset>
          <p className="text-xs text-ink-3">
            Up to 16 steps and 16 parameters. Each input is plain text, up to
            8192 UTF-8 bytes; all inputs together fit 16384 bytes. Save retains
            up to 16 revisions.
          </p>
          <div className="flex gap-2">
            <button
              type="submit"
              className={button}
              disabled={disabled || invalid(draft)}
            >
              Save template
            </button>
            <button
              type="button"
              className={button}
              disabled={busy}
              onClick={() => setDraft(null)}
            >
              Close editor
            </button>
          </div>
          {draft.revision > 0 && (
            <details>
              <summary className="cursor-pointer text-sm text-danger">
                Remove template from picker
              </summary>
              <p className="py-2 text-sm">
                Existing runs and saved retry records remain available.
              </p>
              <button
                type="button"
                className={button}
                disabled={disabled}
                onClick={() =>
                  void submit({
                    type: 'delete',
                    slug: draft.slug,
                    input: { revision: draft.revision },
                  })
                }
              >
                Delete template
              </button>
            </details>
          )}
        </form>
      )}
    </section>
  );
}
