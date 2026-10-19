/** How a provider with several keys picks the next one. */
export type KeyStrategy =
  | 'fill_first'
  | 'round_robin'
  | 'random'
  | 'least_used';

export const KEY_STRATEGIES: readonly KeyStrategy[] = [
  'fill_first',
  'round_robin',
  'random',
  'least_used',
];

/** A provider and model; the unit of a fallback or the side-task model. */
export interface ModelChoice {
  provider: string;
  model: string;
}

/** One key as the daemon lists it. The key itself is never returned. */
export interface ProviderKey {
  /** `env` for the environment key, otherwise `key_` and 12 characters. */
  id: string;
  label: string | null;
  /** The first six and last four characters, for example `sk-pro…abcd`. */
  masked: string;
  source: 'env' | 'file';
  status: 'ready' | 'resting';
  /** When a resting key is tried again, in epoch milliseconds. */
  restingUntilMs: number | null;
  lastError: string | null;
  uses: number;
}

export interface ProviderKeys {
  id: string;
  strategy: KeyStrategy;
  /** False when the daemon has no key file: added keys last until restart. */
  persisted: boolean;
  keys: ProviderKey[];
}

export interface AddProviderKeyInput {
  label?: string;
  key: string;
}
