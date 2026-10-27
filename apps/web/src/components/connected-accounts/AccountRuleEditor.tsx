import { useState } from 'react';
import type {
  AccountEventField,
  AccountInputBinding,
  AccountWorkflowInput,
  AccountWorkflowRule,
  ConnectedSource,
  WorkflowTemplate,
} from '@animaos/sdk';
import { accountButton, providerName } from './presentation';

const fields: { value: AccountEventField; label: string }[] = [
  { value: 'text', label: 'Account content' },
  { value: 'title', label: 'Title / subject' },
  { value: 'sender', label: 'Sender' },
  { value: 'sourceUrl', label: 'Source link' },
  { value: 'externalId', label: 'Item ID' },
  { value: 'collectionId', label: 'Collection ID' },
  { value: 'eventId', label: 'Event ID' },
  { value: 'observedAt', label: 'Observed time' },
];
export function AccountRuleEditor({
  sources,
  templates,
  rule,
  disabled,
  onSave,
  onCancel,
}: {
  sources: ConnectedSource[];
  templates: WorkflowTemplate[];
  rule?: AccountWorkflowRule;
  disabled: boolean;
  onSave: (input: AccountWorkflowInput) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState(rule?.name ?? '');
  const [requestKey] = useState(() => crypto.randomUUID());
  const [connector, setConnector] = useState(
    rule?.connectorId ?? sources[0]?.connectorId ?? '',
  );
  const [slug, setSlug] = useState(
    rule?.templateSlug ?? templates[0]?.slug ?? '',
  );
  const [sender, setSender] = useState(rule?.filters.senderContains ?? '');
  const [title, setTitle] = useState(rule?.filters.titleContains ?? '');
  const [collection, setCollection] = useState(
    rule?.filters.collectionId ?? '',
  );
  const [bindings, setBindings] = useState<Record<string, AccountInputBinding>>(
    rule?.inputs ?? {},
  );
  const source = sources.find((s) => s.connectorId === connector);
  const template = templates.find((t) => t.slug === slug);
  const complete = template?.parameters.every(
    (p) => bindings[p.name] || p.default != null,
  );
  return (
    <form
      aria-label="Account rule editor"
      className="space-y-3 rounded-xl border border-line p-4"
      onSubmit={(event) => {
        event.preventDefault();
        if (!source || !template || !name.trim() || !complete) return;
        const inputs: Record<string, AccountInputBinding> = {};
        for (const parameter of template.parameters)
          if (bindings[parameter.name])
            inputs[parameter.name] = bindings[parameter.name];
        onSave({
          ...(!rule ? { requestKey } : {}),
          name: name.trim(),
          connectorId: source.connectorId,
          eventKind:
            source.kind === 'mail'
              ? 'mailObserved'
              : source.kind === 'file'
                ? 'fileChanged'
                : 'calendarChanged',
          templateSlug: template.slug,
          templateRevision: template.revision,
          filters: {
            ...(sender.trim() && source.kind === 'mail'
              ? { senderContains: sender.trim() }
              : {}),
            ...(title.trim() ? { titleContains: title.trim() } : {}),
            ...(collection.trim() ? { collectionId: collection.trim() } : {}),
          },
          inputs,
          enabled: true,
        });
      }}
    >
      <fieldset disabled={disabled} className="space-y-3">
        <label className="block text-xs">
          Rule name
          <input
            className="field mt-1 w-full"
            value={name}
            onChange={(e) => setName(e.target.value)}
            maxLength={160}
            required
          />
        </label>
        <label className="block text-xs">
          Source account
          <select
            className="field mt-1 w-full"
            value={source?.connectorId ?? ''}
            onChange={(e) => setConnector(e.target.value)}
            required
          >
            <option value="" disabled>
              Choose an enabled account
            </option>
            {sources.map((s) => (
              <option key={s.connectorId} value={s.connectorId}>
                {providerName(s.provider)} · {s.accountLabel}
              </option>
            ))}
          </select>
        </label>
        <p className="text-xs text-ink-3">
          {source?.kind === 'mail'
            ? 'When new inbox mail is observed'
            : source?.kind === 'file'
              ? 'When a selected file changes'
              : 'When calendar activity changes'}
        </p>
        <label className="block text-xs">
          Saved workflow
          <select
            className="field mt-1 w-full"
            value={template?.slug ?? ''}
            onChange={(e) => {
              setSlug(e.target.value);
              setBindings({});
            }}
            required
          >
            <option value="" disabled>
              Choose a saved workflow
            </option>
            {templates.map((t) => (
              <option key={t.slug} value={t.slug}>
                {t.title} · version {t.revision}
              </option>
            ))}
          </select>
        </label>
        <label className="block text-xs">
          Title contains
          <input
            className="field mt-1 w-full"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            maxLength={256}
            placeholder="Optional, for example invoice"
          />
        </label>
        {source?.kind === 'mail' && (
          <label className="block text-xs">
            Sender contains
            <input
              className="field mt-1 w-full"
              value={sender}
              onChange={(e) => setSender(e.target.value)}
              maxLength={256}
              placeholder="Optional"
            />
          </label>
        )}
        <details>
          <summary className="cursor-pointer text-xs">
            Collection filter
          </summary>
          <label className="block text-xs mt-2">
            Collection ID
            <input
              className="field mt-1 w-full"
              value={collection}
              onChange={(e) => setCollection(e.target.value)}
              maxLength={256}
              placeholder="Optional exact calendar or collection ID"
            />
          </label>
        </details>
        {template?.parameters.map((parameter) => {
          const binding = bindings[parameter.name];
          return (
            <div className="space-y-2" key={parameter.name}>
              <label className="block text-xs">
                Value for {parameter.name}
                <select
                  className="field mt-1 w-full"
                  value={
                    binding?.type === 'literal'
                      ? 'literal'
                      : (binding?.field ?? '')
                  }
                  onChange={(e) => {
                    const value = e.target.value;
                    setBindings((current) => {
                      const next = { ...current };
                      if (!value) delete next[parameter.name];
                      else
                        next[parameter.name] =
                          value === 'literal'
                            ? { type: 'literal', value: '' }
                            : {
                                type: 'event',
                                field: value as AccountEventField,
                              };
                      return next;
                    });
                  }}
                >
                  <option value="">
                    {parameter.default != null
                      ? 'Use saved default'
                      : 'Choose event content or a fixed value'}
                  </option>
                  {fields.map((field) => (
                    <option key={field.value} value={field.value}>
                      {field.label}
                    </option>
                  ))}
                  <option value="literal">Fixed value</option>
                </select>
              </label>
              {binding?.type === 'literal' && (
                <label className="block text-xs">
                  Fixed value for {parameter.name}
                  <textarea
                    className="field mt-1 w-full"
                    value={binding.value}
                    maxLength={8192}
                    onChange={(e) =>
                      setBindings((current) => ({
                        ...current,
                        [parameter.name]: {
                          type: 'literal',
                          value: e.target.value,
                        },
                      }))
                    }
                  />
                </label>
              )}
            </div>
          );
        })}
        <p className="text-xs text-ink-3">
          Saving enables this rule with a fresh baseline. Updating the workflow
          or reconnecting its account pauses the rule until you enable it again.
        </p>
        <div className="flex gap-2">
          <button
            type="submit"
            className={accountButton}
            disabled={!source || !template || !name.trim() || !complete}
          >
            Save account rule
          </button>
          <button type="button" className={accountButton} onClick={onCancel}>
            Cancel rule edit
          </button>
        </div>
      </fieldset>
    </form>
  );
}
