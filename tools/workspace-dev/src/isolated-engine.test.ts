import { test, expect } from 'vitest';
import { mkdtemp, readFile, writeFile, rm, mkdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { prepareIsolatedEngine, isolatedEnvironment } from './isolated-engine.js';
test('isolated launcher copies only provider/model into new fixture and preserves material on reopen', async () => {
    const parent = await mkdtemp(join(tmpdir(), 'anima-startup-test-'));
    try {
        const source = join(parent, 'source');
        await mkdir(source);
        const file = join(source, 'control-plane.json');
        await writeFile(file, JSON.stringify({
            agents: [{
                    state: {
                        config: {
                            name: 'Anima',
                            provider: 'chatgpt',
                            model: 'selected-model',
                            system: 'PRIVATE',
                            bio: 'PRIVATE',
                            knowledge: ['PRIVATE']
                        },
                        messages: ['PRIVATE']
                    }
                }],
            connectors: ['PRIVATE'],
            jobs: ['PRIVATE']
        }));
        const root = join(parent, 'fixture');
        const result = await prepareIsolatedEngine(root.split(String.fromCharCode(92)).join('/'), file, source, undefined, 'installed');
        const control = JSON.parse(await readFile(join(root, 'daemon/control-plane.json'), 'utf8'));
        expect(control.version).toBe(30);
        expect(control.agents).toHaveLength(1);
        expect(control.agents[0].state.config.tools).toBeNull();
        expect(control.agents[0].state.config.model).toBe('selected-model');
        expect(JSON.stringify(control)).not.toContain('PRIVATE');
        expect(control.connectors).toEqual([]);
        expect(control.jobs).toEqual([]);
        expect(control.agents[0].state.id).toBe(result.agentId);
        const key = await readFile(join(root, 'authority/daemon.key'));
        expect(key.length).toBe(32);
        const admin = await readFile(join(root, 'authority/daemon.admin-token'), 'utf8');
        expect(admin).toMatch(/^[a-f0-9]{64}$/);
        await expect(prepareIsolatedEngine(root, file, source)).rejects.toThrow(/EEXIST/);
        expect(await readFile(join(root, 'authority/daemon.key'))).toEqual(key);
        expect(await readFile(file, 'utf8')).toContain('PRIVATE');
        const env = isolatedEnvironment(result, {
            VILLAGE_DB_PATH: 'real-save',
            ANIMAOS_RS_MEMORY_FILE: 'real-memory',
            ANIMA_HOME: 'real-home',
            DATABASE_URL: 'real-database',
            PATH: 'fixture-path',
            OPENAI_API_KEY: 'synthetic'
        });
        expect(env.VILLAGE_DB_PATH).toBe(join(root, 'village/saves.sqlite'));
        expect(env.ANIMAOS_RS_MEMORY_FILE).toBeUndefined();
        expect(env.ANIMA_HOME).toBe(root);
        expect(env.DATABASE_URL).toBeUndefined();
        expect(env.ANIMA_LOCAL_ADMIN_TOKEN_FILE).toBe(join(root, 'authority/daemon.admin-token'));
        expect(env.ANIMA_LOCAL_ADMIN_TOKEN).toBeUndefined();
        expect(env.OPENAI_API_KEY).toBe('synthetic');
        expect(env.VILLAGE_KEYCHAIN_NAMESPACE).toBe('installed');
        expect(env.ANIMAOS_RS_KEYCHAIN_NAMESPACE).toBe('installed');
        expect(env.ANIMAOS_RS_MEMORY_EMBEDDINGS).toBe('off');
    }
    finally {
        await rm(parent, {
            recursive: true,
            force: true
        });
    }
});
test('isolated launcher refuses roots within source profile and reserved ports', async () => {
    const parent = await mkdtemp(join(tmpdir(), 'anima-startup-test-'));
    try {
        const file = join(parent, 'control.json');
        await writeFile(file, JSON.stringify({
            agents: [{
                    state: {
                        config: {
                            name: 'Anima',
                            provider: 'chatgpt',
                            model: 'fixture'
                        }
                    }
                }]
        }));
        await expect(prepareIsolatedEngine(join(parent, 'nested'), file, parent)).rejects.toThrow(/fixture root must be separate/);
        await expect(prepareIsolatedEngine(join(parent, 'fixture'), file, join(parent, 'profile'), {
            daemon: 8080,
            village: 29342,
            web: 29343
        })).rejects.toThrow(/three distinct isolated ports/);
    }
    finally {
        await rm(parent, {
            recursive: true,
            force: true
        });
    }
});
