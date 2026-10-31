import { useEffect, useRef, useState } from 'react';

import { daemon, type CodingKey } from '../../lib/daemon-api';
import { Switch } from '../ui-bits';
import {
  SettingsCard,
  SettingsEmpty,
  SettingsRow,
  SettingsSection,
} from '../settings/SettingsKit';
import { useResetOnChange } from '../../hooks/useResetOnChange';

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
  useResetOnChange([agentId], () => {
    setKeys(null);
    setError(null);
  });
  useEffect(() => {
    const current = ++generation.current;
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
    <SettingsSection
      id="coding-keys"
      title="Keys for coding apps"
      description="Coding apps normally use their own sign-in. Turn a key on to hand it to the apps anima starts for this agent. anima never sees the value."
    >
      {error && (
        <p className="approvals-error" role="alert">
          {error}
        </p>
      )}
      {keys === null ? (
        !error && (
          <SettingsCard>
            <SettingsEmpty>Reading keys…</SettingsEmpty>
          </SettingsCard>
        )
      ) : (
        <SettingsCard as="ul" label="Keys for coding apps">
          {keys.map((key) => (
            <SettingsRow
              as="li"
              key={key.name}
              title={key.label}
              description={<code className="coding-key-name">{key.name}</code>}
              control={
                <>
                  <span className="sk-meta">
                    {key.saved ? 'Saved' : 'Not saved'}
                  </span>
                  <Switch
                    checked={key.enabled}
                    disabled={!key.saved || saving}
                    label={`Hand ${key.label} to coding apps`}
                    onChange={(on) => void toggle(key.name, on)}
                  />
                </>
              }
            />
          ))}
        </SettingsCard>
      )}
      <p className="sk-meta coding-keys-note">
        Keys come from Settings › Model. A key that isn't saved can't be turned
        on.
      </p>
    </SettingsSection>
  );
}
