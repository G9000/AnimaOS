import { useEffect, useRef, useState } from 'react';

import { daemon, type CodingKey } from '../../lib/daemon-api';
import { Switch } from '../ui-bits';

function errorText(caught: unknown): string {
  return caught instanceof Error ? caught.message : String(caught);
}

/**
 * Keys for coding apps: the owner hands a saved key to the coding apps this
 * agent starts. Off by default, a fixed set of names, and the value never
 * reaches the page or the model.
 */
export function CodingKeys({ agentId }: { agentId: string }) {
  const [keys, setKeys] = useState<CodingKey[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const generation = useRef(0);

  useEffect(() => {
    const current = ++generation.current;
    setKeys(null);
    setError(null);
    daemon.codingKeys(agentId).then(
      (result) => {
        if (generation.current === current) setKeys(result.keys);
      },
      (caught: unknown) => {
        if (generation.current === current) setError(errorText(caught));
      },
    );
  }, [agentId]);

  const toggle = async (name: string, on: boolean) => {
    if (!keys || saving) return;
    const before = keys;
    const next = keys.map((key) =>
      key.name === name ? { ...key, enabled: on } : key,
    );
    const current = generation.current;
    setKeys(next);
    setSaving(true);
    setError(null);
    try {
      const result = await daemon.setCodingKeys(
        agentId,
        next.filter((key) => key.enabled).map((key) => key.name),
      );
      if (generation.current === current) setKeys(result.keys);
    } catch (caught) {
      if (generation.current === current) {
        setKeys(before);
        setError(errorText(caught));
      }
    } finally {
      if (generation.current === current) setSaving(false);
    }
  };

  return (
    <section className="approvals-section" aria-labelledby="coding-keys">
      <h2 id="coding-keys">Keys for coding apps</h2>
      <p className="approvals-help">
        Coding apps normally use their own sign-in. Turn a key on to hand it to
        apps anima starts for this agent. anima never sees the value.
      </p>
      {error && (
        <p className="approvals-error" role="alert">
          {error}
        </p>
      )}
      {keys === null ? (
        !error && <p className="approvals-empty">Reading keys…</p>
      ) : (
        <ul className="coding-keys" aria-label="Keys for coding apps">
          {keys.map((key) => (
            <li key={key.name} className="coding-key">
              <span className="coding-key-icon" aria-hidden="true">
                ⚿
              </span>
              <span className="coding-key-label">{key.label}</span>
              <code className="coding-key-name">{key.name}</code>
              <span className="coding-key-state">
                {key.saved ? 'Saved' : 'Not saved'}
              </span>
              <Switch
                checked={key.enabled}
                disabled={!key.saved || saving}
                label={`Hand ${key.label} to coding apps`}
                onChange={(on) => void toggle(key.name, on)}
              />
            </li>
          ))}
        </ul>
      )}
      <p className="approvals-help coding-keys-note">
        <span aria-hidden="true">↳</span> Keys come from Settings › Model. A
        key that isn't saved can't be turned on.
      </p>
    </section>
  );
}
