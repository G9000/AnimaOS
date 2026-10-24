import { describe, expect, it } from 'vitest';

import { rustDaemonTestEnvironment } from './rust-daemon-tests.js';

describe('Rust daemon test owner environment', () => {
  it('replaces inherited owner credentials and bootstrap overrides without changing the caller', () => {
    const inherited = {
      ANIMA_LOCAL_ADMIN_TOKEN: 'real-owner-token',
      ANIMA_LOCAL_ADMIN_TOKEN_FILE: 'real-owner-token-file',
      ANIMAOS_TEST_OWNER_TOKEN: 'stale-fixture-token',
      ANIMAOS_RS_API_KEY: 'real-api-key',
      ANIMA_ALLOWED_UI_ORIGINS: 'https://console.example',
      ANIMA_SUPERVISOR_URL: 'http://127.0.0.1:9999',
      ANIMA_SUPERVISOR_TOKEN: 'real-supervisor-token',
      ANIMA_SUPERVISOR_TOKEN_FILE: 'real-supervisor-token-file',
      ANIMAOS_RS_HOST: '0.0.0.0',
      ANIMAOS_ALLOW_HOST_EXECUTION: '1',
      ANIMAOS_EXECUTION_PROFILES_FILE: 'operator-execution-profiles',
      animaos_execution_profiles_file: 'mixed-case-operator-execution-profiles',
      anima_local_admin_token_file: 'mixed-case-owner-file',
      CI: '1',
      CARGO_TARGET_DIR: 'target/cargo/validation-rust-daemon',
      PATH: 'cargo-path',
      TMPDIR: 'owned-task-temp',
    };
    const before = { ...inherited };

    const isolated = rustDaemonTestEnvironment(inherited);

    expect(isolated.ANIMAOS_TEST_OWNER_TOKEN).toMatch(/^[a-f0-9]{64}$/);
    expect(isolated.ANIMA_LOCAL_ADMIN_TOKEN).toBe(isolated.ANIMAOS_TEST_OWNER_TOKEN);
    const unchangedEnvironment = Object.fromEntries(
      Object.entries(inherited).filter(([name]) => !/^anima/i.test(name)),
    );
    expect(isolated).toEqual({
      ...unchangedEnvironment,
      ANIMA_LOCAL_ADMIN_TOKEN: isolated.ANIMAOS_TEST_OWNER_TOKEN,
      ANIMAOS_TEST_OWNER_TOKEN: expect.stringMatching(/^[a-f0-9]{64}$/),
    });
    expect(inherited).toEqual(before);
  });

  it('uses a fresh synthetic key for each Cargo invocation', () => {
    const first = rustDaemonTestEnvironment({});
    const second = rustDaemonTestEnvironment({});

    expect(first.ANIMAOS_TEST_OWNER_TOKEN).not.toBe(second.ANIMAOS_TEST_OWNER_TOKEN);
    expect(second.ANIMA_LOCAL_ADMIN_TOKEN).toBe(second.ANIMAOS_TEST_OWNER_TOKEN);
  });
});
