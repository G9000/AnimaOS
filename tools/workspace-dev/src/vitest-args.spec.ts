import { describe, expect, it } from 'vitest';

import { normalizeVitestArgs } from './vitest-args.js';

describe('normalizeVitestArgs', () => {
  it('removes an explicit run verb before the shared runner adds it', () => {
    expect(normalizeVitestArgs(['run', 'src/example.spec.ts'])).toEqual([
      'src/example.spec.ts',
    ]);
  });
  it('strips the Jest-only runInBand flag', () => {
    expect(normalizeVitestArgs(['--runInBand'])).toEqual([]);
  });

  it('strips runInBand with an explicit value', () => {
    expect(normalizeVitestArgs(['--runInBand=true'])).toEqual([]);
  });

  it('strips runInBand with its value as a separate argument', () => {
    expect(normalizeVitestArgs(['--runInBand', 'false'])).toEqual([]);
  });

  it('preserves all other Vitest arguments', () => {
    expect(
      normalizeVitestArgs([
        '--coverage',
        '--runInBand',
        '--watch',
        '--testNamePattern=workspace-dev',
      ]),
    ).toEqual(['--coverage', '--watch', '--testNamePattern=workspace-dev']);
  });
});
