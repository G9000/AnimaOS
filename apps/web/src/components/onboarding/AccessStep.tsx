import { ACCESS_PROFILES, type AccessProfile } from '../../lib/agent-access';

export type OnboardingAutonomy = 'free' | 'strict';

export interface AccessStepProps {
  access: AccessProfile;
  onAccessChange(access: AccessProfile): void;
  /** How much it asks; shown when given. */
  autonomy?: OnboardingAutonomy;
  onAutonomyChange?(autonomy: OnboardingAutonomy): void;
}

const AUTONOMY_OPTIONS: readonly {
  value: OnboardingAutonomy;
  label: string;
  summary: string;
}[] = [
  {
    value: 'free',
    label: 'Free (recommended)',
    summary:
      'Works on its own and tells you what it did. Dangerous things still ask.',
  },
  {
    value: 'strict',
    label: 'Strict',
    summary: 'Asks before it changes or runs anything.',
  },
];

const ACCESS_ORDER: readonly AccessProfile[] = [
  'observe',
  'collaborate',
  'operate',
];

export function AccessStep({
  access,
  onAccessChange,
  autonomy,
  onAutonomyChange,
}: AccessStepProps) {
  return (
    <section aria-labelledby="onboarding-access-heading" className="space-y-5">
      <div>
        <h2
          id="onboarding-access-heading"
          className="font-display text-2xl font-semibold tracking-tight text-ink"
        >
          Access
        </h2>
        <p className="mt-1 text-sm text-ink-2">
          Choose what your companion can inspect, change, and run. Any helpers
          stay within these same tool permissions.
        </p>
      </div>

      <fieldset className="space-y-2">
        <legend className="sr-only">Access profile</legend>
        {ACCESS_ORDER.map((profileName) => {
          const profile = ACCESS_PROFILES[profileName];
          const inputId = `onboarding-access-${profileName}`;

          return (
            <div
              key={profileName}
              className={`rounded-xl border p-4 transition ${
                access === profileName
                  ? 'border-accent/55 bg-accent/[0.07] shadow-[0_14px_36px_-30px_rgb(var(--color-accent-rgb)/0.75)]'
                  : 'border-line bg-white/[0.015] hover:border-line-strong'
              }`}
            >
              <input
                id={inputId}
                type="radio"
                name="onboarding-access"
                value={profileName}
                checked={access === profileName}
                onChange={() => onAccessChange(profileName)}
                className="mr-3 h-4 w-4 align-top"
              />
              <label htmlFor={inputId} className="inline cursor-pointer">
                <span className="font-medium text-ink">{profile.label}</span>
                <span className="mt-1 block pl-7 text-sm text-ink-2">
                  {profile.summary}
                </span>
                <span className="mt-1 block pl-7 text-xs text-ink-3">
                  {profile.risk}
                </span>
              </label>
            </div>
          );
        })}
      </fieldset>

      {autonomy && onAutonomyChange && (
        <fieldset className="space-y-2">
          <legend className="text-sm font-medium text-ink">
            How much should it ask?
          </legend>
          {AUTONOMY_OPTIONS.map((option) => {
            const inputId = `onboarding-autonomy-${option.value}`;
            return (
              <div
                key={option.value}
                className={`rounded-xl border p-4 transition ${
                  autonomy === option.value
                    ? 'border-accent/55 bg-accent/[0.07]'
                    : 'border-line bg-white/[0.015] hover:border-line-strong'
                }`}
              >
                <input
                  id={inputId}
                  type="radio"
                  name="onboarding-autonomy"
                  value={option.value}
                  checked={autonomy === option.value}
                  onChange={() => onAutonomyChange(option.value)}
                  className="mr-3 h-4 w-4 align-top"
                />
                <label htmlFor={inputId} className="inline cursor-pointer">
                  <span className="font-medium text-ink">{option.label}</span>
                  <span className="mt-1 block pl-7 text-sm text-ink-2">
                    {option.summary}
                  </span>
                </label>
              </div>
            );
          })}
        </fieldset>
      )}
    </section>
  );
}
