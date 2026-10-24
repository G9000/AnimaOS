import { execFile, spawn, type ChildProcess } from 'node:child_process';
import {
  mkdtemp,
  mkdir,
  readFile,
  rm,
  stat,
  writeFile,
} from 'node:fs/promises';
import { createServer } from 'node:http';
import { createServer as createNetServer } from 'node:net';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createDaemonClient } from '../src/index.js';
import {
  DAEMON_TEST_OWNER_TOKEN,
  daemonTestEnvironment,
} from './daemon-environment.js';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const target = resolve(
  repository,
  process.env.CARGO_TARGET_DIR ?? 'target/cargo/sdk-daemon-integration',
);
// A supplied artifact permits acceptance of an already-built executable.
// Ordinary test runs always compile the current tree first.
const suppliedBinary = process.env.ANIMA_PERSONALITY_TEST_BINARY;
const binary = suppliedBinary
  ? resolve(suppliedBinary)
  : join(
      target,
      'debug',
      process.platform === 'win32' ? 'anima-daemon.exe' : 'anima-daemon',
    );

describe.skipIf(process.env.ANIMA_SKIP_SERVICE_TESTS === '1')(
  'personality persistence in a real daemon process',
  { concurrent: false },
  () => {
    let directory: string;
    let daemon: ChildProcess | undefined;
    let output = '';
    let port: number;
    const ownerToken = DAEMON_TEST_OWNER_TOKEN;
    let client = createDaemonClient({ ownerToken });
    const requests: { messages?: { role: string; content: unknown }[] }[] = [];
    const model = createServer(async (request, response) => {
      if (request.url !== '/v1/chat/completions') {
        response.writeHead(404).end();
        return;
      }
      let body = '';
      for await (const chunk of request) body += String(chunk);
      requests.push(JSON.parse(body));
      response.setHeader('content-type', 'application/json');
      response.end(
        JSON.stringify({
          choices: [
            {
              message: {
                role: 'assistant',
                content: 'Personality test acknowledged.',
              },
              finish_reason: 'stop',
            },
          ],
          usage: { prompt_tokens: 10, completion_tokens: 4, total_tokens: 14 },
        }),
      );
    });
    let modelUrl: string;

    async function startDaemon() {
      output = '';
      daemon = spawn(binary, [], {
        cwd: directory,
        windowsHide: true,
        stdio: ['ignore', 'pipe', 'pipe'],
        env: {
          ...daemonTestEnvironment(process.env, directory, port),
          ANIMAOS_RS_CONTROL_PLANE_FILE: join(directory, 'control-plane.json'),
          ANIMAOS_RS_KEYCHAIN_NAMESPACE: `persona-test-${port}`,
          ANIMA_LOCAL_ADMIN_TOKEN: ownerToken,
          OPENAI_API_KEY: 'isolated-personality-test-key',
          OPENAI_BASE_URL: modelUrl,
        },
      });
      daemon.stdout?.on('data', (chunk) => {
        output += String(chunk);
      });
      daemon.stderr?.on('data', (chunk) => {
        output += String(chunk);
      });
      const deadline = Date.now() + 15000;
      while (Date.now() < deadline) {
        if (daemon.exitCode !== null)
          throw new Error(`Test daemon exited: ${output}`);
        try {
          if ((await client.health()).status === 'ok') return;
        } catch {
          /* startup */
        }
        await new Promise((resolveDelay) => setTimeout(resolveDelay, 100));
      }
      throw new Error(`Test daemon did not become healthy: ${output}`);
    }
    async function stopDaemon() {
      if (!daemon || daemon.exitCode !== null) return;
      const child = daemon;
      await new Promise<void>((resolveStop, reject) => {
        const timer = setTimeout(
          () => reject(new Error('Test daemon did not stop')),
          10000,
        );
        child.once('exit', () => {
          clearTimeout(timer);
          resolveStop();
        });
        child.kill();
      });
      daemon = undefined;
    }

    beforeAll(async () => {
      if (!suppliedBinary)
        await promisify(execFile)(
          'cargo',
          [
            'build',
            '--manifest-path',
            join(repository, 'Cargo.toml'),
            '-p',
            'anima-daemon',
            '--target-dir',
            target,
          ],
          {
            cwd: repository,
            windowsHide: true,
            timeout: 300000,
            maxBuffer: 8 * 1024 * 1024,
          },
        );
      expect((await stat(binary)).isFile()).toBe(true);
      directory = await mkdtemp(join(tmpdir(), 'anima-personality-test-'));
      await new Promise<void>((resolveListen) =>
        model.listen(0, '127.0.0.1', resolveListen),
      );
      const address = model.address();
      if (!address || typeof address === 'string')
        throw new Error('No test model port');
      modelUrl = `http://127.0.0.1:${address.port}/v1`;
      const listener = createNetServer();
      port = await new Promise<number>((resolvePort, reject) => {
        listener.once('error', reject);
        listener.listen(0, '127.0.0.1', () => {
          const address = listener.address();
          if (!address || typeof address === 'string') {
            reject(new Error('No test daemon port'));
            return;
          }
          listener.close(() => resolvePort(address.port));
        });
      });
      client = createDaemonClient({
        baseUrl: `http://127.0.0.1:${port}`,
        ownerToken,
      });
      await startDaemon();
    }, 360000);

    afterAll(async () => {
      await stopDaemon();
      if (model.listening)
        await new Promise<void>((resolveClose, reject) =>
          model.close((error) => (error ? reject(error) : resolveClose())),
        );
      if (
        directory &&
        dirname(resolve(directory)) === resolve(tmpdir()) &&
        basename(directory).startsWith('anima-personality-test-')
      )
        await rm(directory, { recursive: true, force: true });
    });

    it('uses saved config and profile SOUL in separate chats before and after a process restart', async () => {
      await client.workspace.put({
        rootPath: directory,
        companyName: 'Personality test',
        mission: 'Isolated persistence verification',
        values: [],
      });
      const configured = await client.agents.create({
        name: 'Config persona',
        model: 'personality-test',
        provider: 'openai',
        system: 'DISPLACED_CONFIG_PERSONALITY',
        settings: { autoTitle: false },
        tools: [],
      });
      const soul = await client.agents.create({
        name: 'File persona',
        model: 'personality-test',
        provider: 'openai',
        system: 'DISPLACED_FILE_PERSONALITY',
        tools: [],
        settings: { agentFilesProfileEnabled: true, autoTitle: false },
      });
      const profilePath = join(
        directory,
        '.animaos-swarm',
        'agent-profiles',
        Buffer.from(soul.state.id).toString('hex'),
      );
      await mkdir(profilePath, { recursive: true });
      await writeFile(
        join(profilePath, 'SOUL.md'),
        '# Identity\nOriginal file personality.',
      );
      const cases = [
        {
          id: configured.state.id,
          source: 'agent_config',
          old: 'DISPLACED_CONFIG_PERSONALITY',
          content:
            '# Identity and role\nYou are a curious researcher.\n\n## Values and philosophy\nFollow evidence. CONFIG_PERSONALITY_PERSISTED',
        },
        {
          id: soul.state.id,
          source: 'profile_soul',
          old: 'DISPLACED_FILE_PERSONALITY',
          content:
            '# Identity and role\nYou are a thoughtful companion.\n\n## Communication\nBe clear. FILE_PERSONALITY_PERSISTED',
        },
      ];
      for (const entry of cases) {
        const original = await client.agentFiles.personality(entry.id);
        expect(original.source).toBe(entry.source);
        const saved = await client.agentFiles.savePersonality(entry.id, {
          revision: original.revision,
          content: entry.content,
        });
        expect(saved.content).toBe(entry.content);
      }
      expect(await readFile(join(profilePath, 'SOUL.md'), 'utf8')).toBe(
        cases[1].content,
      );
      for (const phase of ['before-restart', 'after-restart']) {
        if (phase === 'after-restart') {
          await stopDaemon();
          await startDaemon();
        }
        for (const entry of cases) {
          expect((await client.agentFiles.personality(entry.id)).content).toBe(
            entry.content,
          );
          for (let chat = 0; chat < 2; chat++) {
            const firstRequest = requests.length;
            const turnText = `Hello ${phase} chat ${chat}`;
            await client.agents.run(
              entry.id,
              { text: turnText },
              { roomId: `personality-${phase}-${entry.source}-${chat}` },
            );
            const sent = requests
              .slice(firstRequest)
              .filter((request) =>
                request.messages?.some(
                  (message) =>
                    message.role === 'user' && message.content === turnText,
                ),
              )
              .flatMap((request) => request.messages ?? [])
              .filter((message) => message.role === 'system');
            expect(sent.length).toBeGreaterThan(0);
            for (const message of sent) {
              expect(message.content).toEqual(
                expect.stringContaining(entry.content),
              );
              expect(message.content).not.toEqual(
                expect.stringContaining(entry.old),
              );
            }
          }
        }
      }
    }, 60000);
  },
);
