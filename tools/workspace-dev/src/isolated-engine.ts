import { mkdir, open, readFile, realpath, lstat } from 'node:fs/promises';
import { randomBytes, randomUUID } from 'node:crypto';
import { dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { createServer } from 'node:net';
export type IsolatedPorts = {
    daemon: number;
    village: number;
    web: number;
};
export type IsolatedEngine = {
    version: 1;
    root: string;
    sourceProfile: string;
    provider: string;
    model: string;
    agentId: string;
    keychainNamespace?: string;
    ports: IsolatedPorts;
};
const defaults: IsolatedPorts = {
    daemon: 29341,
    village: 29342,
    web: 29343
};
function validatePorts(ports: IsolatedPorts) {
    const values = Object.values(ports);
    if (new Set(values).size !== 3 || values.some(p => !Number.isInteger(p) || p < 1024 || p > 65535 || [8080, 24302, 28192].includes(p)))
        throw new Error('Use three distinct isolated ports outside the normal application ports.');
}
function overlaps(a: string, b: string) {
    const normalized = (s: string) => process.platform === 'win32' ? s.toLowerCase() : s;
    const contained = (x: string, y: string) => {
        const r = relative(normalized(x), normalized(y));
        return !r || (!r.startsWith('..') && !isAbsolute(r));
    };
    return contained(a, b) || contained(b, a);
}
async function exclusive(path: string, value: string | Uint8Array) {
    const file = await open(path, 'wx', 0o600);
    try {
        await file.writeFile(value);
        await file.sync();
    }
    finally {
        await file.close();
    }
}
export async function prepareIsolatedEngine(root: string, sourceFile: string, sourceProfile: string, ports: IsolatedPorts = defaults, keychainNamespace?: string): Promise<IsolatedEngine> {
    validatePorts(ports);
    if (keychainNamespace !== undefined && !/^[a-z0-9-]{1,32}$/.test(keychainNamespace))
        throw new Error('Invalid existing credential namespace.');
    if (!isAbsolute(root))
        throw new Error('Use an absolute fresh fixture root.');
    root = resolve(root);
    const parent = await realpath(dirname(root));
    root = join(parent, root.slice(dirname(root).length + 1));
    const profile = await realpath(sourceProfile);
    const source = await realpath(sourceFile);
    if (overlaps(root, profile) || overlaps(root, source))
        throw new Error('The fixture root must be separate from the existing profile.');
    const stat = await lstat(source);
    if (!stat.isFile() || stat.size > 64 * 1024 * 1024)
        throw new Error('The source model configuration is unavailable.');
    const input = JSON.parse(await readFile(source, 'utf8')) as {
        agents?: {
            state?: {
                config?: {
                    name?: string;
                    provider?: string;
                    model?: string;
                };
            };
        }[];
    };
    const selected = input.agents?.find(a => a.state?.config?.name === 'Anima') ?? (input.agents?.length === 1 ? input.agents[0] : undefined);
    const provider = selected?.state?.config?.provider;
    const model = selected?.state?.config?.model;
    if (!provider?.trim() || !model?.trim() || provider.length > 128 || model.length > 256)
        throw new Error('Select a source configuration with an existing Anima provider and model.');
    await mkdir(root, {
        mode: 0o700
    });
    for (const name of ['authority', 'daemon', 'village', 'runtime'])
        await mkdir(join(root, name), {
            mode: 0o700
        });
    await exclusive(join(root, 'authority', 'daemon.admin-token'), randomBytes(32).toString('hex'));
    const agentId = randomUUID();
    const manifest: IsolatedEngine = {
        version: 1,
        root,
        sourceProfile: profile,
        provider,
        model,
        agentId,
        keychainNamespace,
        ports
    };
    for (const host of ['daemon', 'village']) {
        await exclusive(join(root, 'authority', host + '.owner'), randomUUID());
        await exclusive(join(root, 'authority', host + '.key'), randomBytes(32));
    }
    const paths = (host: string) => ({
        execution: join(root, host, 'execution.sqlite'),
        memory: join(root, host, 'canonical-memory.sqlite'),
        owner: join(root, 'authority', host + '.owner'),
        key: join(root, 'authority', host + '.key')
    });
    const config = {
        name: 'Isolated conversation fixture',
        provider,
        model,
        system: 'You are a temporary conversation fixture. Answer briefly and remember only statements accepted in this fixture.',
        bio: null,
        lore: null,
        knowledge: null,
        topics: null,
        adjectives: null,
        style: null,
        tools: null,
        plugins: null,
        settings: null
    };
    const snapshot = {
        state: {
            id: agentId,
            name: config.name,
            status: 'idle',
            config,
            created_at_ms: Date.now(),
            token_usage: {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
                cached_prompt_tokens: 0,
                reasoning_tokens: 0
            }
        },
        message_count: 0,
        messages: [],
        event_count: 0,
        events: [],
        last_task: null,
        step_count: 0
    };
    await exclusive(join(root, 'daemon', 'control-plane.json'), JSON.stringify({
        version: 30,
        agents: [snapshot],
        swarms: [],
        connectors: [],
        jobs: [],
        schedules: [],
        runs: [],
        sessions: [],
        approvals: [],
        inbound: [],
        outbound: [],
        driveRecords: [],
        discordConnectors: [],
        calendarConnectors: [],
        mailRecords: [],
        workspace: null
    }));
    await exclusive(join(root, 'daemon', 'durable.json'), JSON.stringify({
        version: 1,
        storage: paths('daemon'),
        outbox: join(root, 'daemon', 'outbox.sqlite'),
        profiles: [{
                agent_id: agentId,
                definition_version: 1,
                mode: 'active'
            }]
    }));
    await exclusive(join(root, 'village', 'durable.json'), JSON.stringify({
        version: 1,
        storage: paths('village'),
        saves: join(root, 'village', 'saves.sqlite'),
        mode: 'active'
    }));
    await exclusive(join(root, 'isolated-engine.json'), JSON.stringify(manifest));
    return manifest;
}
export async function readIsolatedEngine(root: string): Promise<IsolatedEngine> {
    root = await realpath(root);
    const manifest = JSON.parse(await readFile(join(root, 'isolated-engine.json'), 'utf8')) as IsolatedEngine;
    if (manifest.version !== 1 || manifest.root !== root || overlaps(root, manifest.sourceProfile) || !manifest.provider?.trim() || !manifest.model?.trim())
        throw new Error('Invalid isolated fixture manifest.');
    validatePorts(manifest.ports);
    if (manifest.keychainNamespace !== undefined && !/^[a-z0-9-]{1,32}$/.test(manifest.keychainNamespace))
        throw new Error('Invalid existing credential namespace.');
    return manifest;
}
export function isolatedEnvironment(fixture: IsolatedEngine, inherited: NodeJS.ProcessEnv = process.env): NodeJS.ProcessEnv {
    const env = Object.fromEntries(Object.entries(inherited).filter(([name]) => !/^ANIMA(?:OS|_)/i.test(name) && !/^VILLAGE_/i.test(name) && !['DATABASE_URL', 'NODE_OPTIONS', 'BUN_OPTIONS'].includes(name.toUpperCase())));
    const root = fixture.root;
    return {
        ...env,
        ...(fixture.keychainNamespace ? {
            VILLAGE_KEYCHAIN_NAMESPACE: fixture.keychainNamespace,
            ANIMAOS_RS_KEYCHAIN_NAMESPACE: fixture.keychainNamespace
        } : {}),
        ANIMA_HOME: root,
        ANIMA_LOCAL_ADMIN_TOKEN_FILE: join(root, 'authority', 'daemon.admin-token'),
        ANIMAOS_RS_HOST: '127.0.0.1',
        ANIMAOS_RS_PORT: String(fixture.ports.daemon),
        ANIMAOS_RS_PERSISTENCE_MODE: 'memory',
        ANIMAOS_RS_MEMORY_EMBEDDINGS: 'off',
        ANIMAOS_RS_CONTROL_PLANE_FILE: join(root, 'daemon', 'control-plane.json'),
        ANIMAOS_RS_MEMORY_SQLITE_FILE: join(root, 'daemon', 'legacy-memory.sqlite'),
        ANIMAOS_RS_MEMORY_EMBEDDINGS_SQLITE_FILE: join(root, 'daemon', 'embeddings.sqlite'),
        ANIMAOS_RS_HISTORY_SQLITE_FILE: join(root, 'daemon', 'history.sqlite'),
        ANIMAOS_RS_PROVIDER_KEYS_FILE: join(root, 'daemon', 'provider-keys.json'),
        ANIMAOS_RS_DURABLE_PROFILE_FILE: join(root, 'daemon', 'durable.json'),
        ANIMAOS_RS_TOOL_RESULTS_DIR: join(root, 'daemon', 'tool-results'),
        VILLAGE_DURABLE_CONFIG_FILE: join(root, 'village', 'durable.json'),
        VILLAGE_DB_PATH: join(root, 'village', 'saves.sqlite'),
        VILLAGE_ANIMA_PROFILE: fixture.sourceProfile,
        VILLAGE_PROVIDER: fixture.provider,
        VILLAGE_MODEL: fixture.model,
        VILLAGE_BIND: '127.0.0.1:' + fixture.ports.village,
        VILLAGE_API_PORT: String(fixture.ports.village),
        VILLAGE_WEB_PORT: String(fixture.ports.web),
        VILLAGE_API_ORIGIN: 'http://127.0.0.1:' + fixture.ports.village
    };
}
export async function checkIsolatedPort(port: number) {
    await new Promise<void>((resolve, reject) => {
        const server = createServer();
        server.once('error', () => reject(new Error('The isolated port is already in use.')));
        server.listen(port, '127.0.0.1', () => server.close(error => error ? reject(error) : resolve()));
    });
}
