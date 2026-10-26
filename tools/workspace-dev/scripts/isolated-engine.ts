import { spawn } from 'node:child_process';
import { resolve, join, isAbsolute } from 'node:path';
import { fileURLToPath } from 'node:url';
import { prepareIsolatedEngine, readIsolatedEngine, isolatedEnvironment, checkIsolatedPort } from '../src/isolated-engine.js';
const repo = resolve(fileURLToPath(new URL('../../..', import.meta.url)));
const [command, ...args] = process.argv.slice(2);
const options = new Map<string, string>();
for (let i = 0; i < args.length; i += 2) {
    if (!args[i]?.startsWith('--') || !args[i + 1])
        throw new Error('Options require a value.');
    options.set(args[i]!, args[i + 1]!);
}
const root = options.get('--root');
if (!root || !isAbsolute(root))
    throw new Error('Pass --root with an absolute isolated fixture directory.');
if (command === 'prepare') {
    const source = options.get('--source-control-plane');
    const profile = options.get('--source-profile');
    if (!source || !profile)
        throw new Error('Pass --source-control-plane and --source-profile for read-only provider/model selection.');
    const fixture = await prepareIsolatedEngine(root, source, profile, undefined, options.get('--keychain-namespace'));
    console.log(JSON.stringify({
        root: fixture.root,
        agentId: fixture.agentId,
        provider: fixture.provider,
        model: fixture.model,
        ports: fixture.ports
    }));
}
else {
    const fixture = await readIsolatedEngine(root);
    const suffix = process.platform === 'win32' ? '.exe' : '';
    let binary: string;
    let extra: string[] = [];
    if (command === 'daemon') {
        await checkIsolatedPort(fixture.ports.daemon);
        binary = options.get('--binary') ?? join(repo, 'target/cargo/combined-validation/debug/anima-daemon' + suffix);
    }
    else if (command === 'village' || command === 'status') {
        if (command === 'village')
            await checkIsolatedPort(fixture.ports.village);
        binary = options.get('--binary') ?? join(repo, 'target/cargo/combined-village/debug/anima-village-game' + suffix);
        if (command === 'status')
            extra = ['--connection-status'];
    }
    else
        throw new Error('Use prepare, daemon, village or status. Startup never makes a provider request automatically.');
    if (!isAbsolute(binary))
        throw new Error('Use an absolute prebuilt binary path.');
    const child = spawn(binary, extra, {
        cwd: join(root, 'runtime'),
        env: isolatedEnvironment(fixture),
        stdio: 'inherit',
        windowsHide: true
    });
    child.once('error', error => {
        console.error(error.message);
        process.exitCode = 1;
    });
    child.once('exit', code => {
        process.exitCode = code ?? 1;
    });
    for (const signal of ['SIGINT', 'SIGTERM'] as const)
        process.once(signal, () => {
            // Windows console children receive Ctrl+C themselves. Node kill() would terminate them before Rust can drain.
            if (process.platform !== 'win32')
                child.kill(signal);
        });
}
