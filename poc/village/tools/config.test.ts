import { expect, it } from 'vitest';
import { resolve } from 'node:path';
import { villageDefaults, villageProcessDefinitions } from './config.js';

it('shares the Rust API origin with Vite without changing the assistant host', () => {
  const processes = villageProcessDefinitions('C:/game', {
    VILLAGE_API_PORT: '8188',
    VILLAGE_WEB_PORT: '4399',
  });
  expect(processes).toHaveLength(2);
  expect(processes[0]?.args).toContain('anima-village-game');
  expect(processes[0]?.args).toEqual([
    'run',
    '--manifest-path',
    resolve('C:/game', 'Cargo.toml'),
    '-p',
    'anima-village-game',
  ]);
  expect(processes[0]?.cwd).toBe('C:/game');
  expect(processes[0]?.env?.CARGO_TARGET_DIR).toBe(
    resolve('C:/game', '../../target/cargo'),
  );
  expect(processes[0]?.env?.VILLAGE_DB_PATH).toBe(
    resolve('C:/game', '.anima-village/village.sqlite3'),
  );
  expect(processes[1]?.args).toEqual(['run', 'dev:web']);
  expect(processes[0]?.env?.VILLAGE_BIND).toBe('127.0.0.1:8188');
  expect(processes[1]?.env?.VILLAGE_API_ORIGIN).toBe('http://127.0.0.1:8188');
  expect(processes[1]?.env?.VILLAGE_WEB_PORT).toBe('4399');
  const defaults = villageProcessDefinitions('/game', {
    CARGO_TARGET_DIR: '/cache',
  });
  expect(defaults[0]?.env?.VILLAGE_BIND).toBe(
    `127.0.0.1:${villageDefaults.apiPort}`,
  );
  expect(defaults[0]?.env?.CARGO_TARGET_DIR).toBe('/cache');
  expect(defaults[1]?.env?.VILLAGE_WEB_PORT).toBe(
    String(villageDefaults.webPort),
  );
  expect(defaults[1]?.env?.VILLAGE_API_ORIGIN).toBe(villageDefaults.apiOrigin);
});

it('rejects invalid or conflicting listener ports before spawning', () => {
  expect(() =>
    villageProcessDefinitions('/game', { VILLAGE_API_PORT: 'abc' }),
  ).toThrow(/port/i);
  expect(() =>
    villageProcessDefinitions('/game', {
      VILLAGE_API_PORT: '4302',
      VILLAGE_WEB_PORT: '4302',
    }),
  ).toThrow(/different/i);
});
