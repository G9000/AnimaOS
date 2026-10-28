import { execFile, spawn, type ChildProcess } from 'node:child_process';
import {
  lstat,
  mkdtemp,
  readFile,
  realpath,
  rm,
  stat,
  utimes,
  writeFile,
} from 'node:fs/promises';
import { createServer } from 'node:http';
import { createServer as createNetServer } from 'node:net';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { beforeAll, describe, expect, it } from 'vitest';
import { createDaemonClient } from '../src/index.js';
import {
  DAEMON_TEST_OWNER_TOKEN,
  daemonTestEnvironment,
} from './daemon-environment.js';
import {
  createPrivateTestHome,
  preparePrivateTestHome,
  type PrivateTestHome,
} from './private-test-home.js';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const target = resolve(
  repository,
  process.env.CARGO_TARGET_DIR ?? 'target/cargo/validation',
);
const binary = join(
  target,
  'debug',
  process.platform === 'win32' ? 'anima-daemon.exe' : 'anima-daemon',
);
const sourcePrefix = 'anima-knowledge-process-';

describe.skipIf(process.env.ANIMA_SKIP_SERVICE_TESTS === '1')(
  'project knowledge in a real daemon process',
  { concurrent: false },
  () => {
    beforeAll(async () => {
      await preparePrivateTestHome();
      // Build time is separate from the bounded process acceptance case.
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
          timeout: 300_000,
          maxBuffer: 8 * 1024 * 1024,
        },
      );
      expect((await stat(binary)).isFile()).toBe(true);
    }, 360_000);

    it('persists a shared folder grant and verified citations across restart, then rejects same-length stale bytes', async () => {
      let privateHome: PrivateTestHome | undefined;
      let sourceDirectory: string | undefined;
      let sourceIdentity: { dev: bigint; ino: bigint } | undefined;
      let temporaryParent: string | undefined;
      let daemon: ChildProcess | undefined;
      let daemonOutput = '';
      let failure: unknown;
      let modelUrl = '';
      let port = 0;
      // Leave time within the test budget for owned process and directory cleanup.
      const runtimeDeadline = Date.now() + 90_000;
      const requests: { messages?: { role: string; content: unknown }[] }[] =
        [];
      const model = createServer(async (request, response) => {
        if (
          request.method !== 'POST' ||
          request.url !== '/v1/chat/completions'
        ) {
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
                  content: 'Knowledge evidence acknowledged.',
                },
                finish_reason: 'stop',
              },
            ],
            usage: {
              prompt_tokens: 10,
              completion_tokens: 4,
              total_tokens: 14,
            },
          }),
        );
      });
      let client = createDaemonClient({ ownerToken: DAEMON_TEST_OWNER_TOKEN });

      async function startDaemon() {
        if (!privateHome) throw new Error('No private test profile');
        const child = spawn(binary, [], {
          cwd: privateHome.workspace,
          windowsHide: true,
          stdio: ['ignore', 'pipe', 'pipe'],
          env: {
            ...daemonTestEnvironment(process.env, privateHome.workspace, port),
            ANIMAOS_RS_CONTROL_PLANE_FILE: join(
              privateHome.profile,
              'control-plane.json',
            ),
            ANIMAOS_RS_KEYCHAIN_NAMESPACE: `knowledge-process-test-${port}`,
            OPENAI_API_KEY: 'isolated-knowledge-test-key',
            OPENAI_BASE_URL: modelUrl,
          },
        });
        daemon = child;
        daemonOutput += `\nStarting owned daemon ${String(child.pid)}\n`;
        let spawnFailure: Error | undefined;
        child.once('error', (error) => {
          spawnFailure = error;
        });
        child.stdout?.on('data', (chunk) => {
          daemonOutput += String(chunk);
        });
        child.stderr?.on('data', (chunk) => {
          daemonOutput += String(chunk);
        });
        const deadline = Math.min(Date.now() + 15_000, runtimeDeadline);
        while (Date.now() < deadline) {
          if (spawnFailure) throw spawnFailure;
          if (child.exitCode !== null || child.signalCode !== null)
            throw new Error(
              `Owned daemon exited before readiness: ${daemonOutput}`,
            );
          try {
            const health = await client.requestJson<{ status: string }>(
              '/health',
              {
                signal: AbortSignal.timeout(1_000),
              },
            );
            if (health.status === 'ok') return;
          } catch {
            /* startup */
          }
          await new Promise((resolveDelay) => setTimeout(resolveDelay, 100));
        }
        throw new Error(`Owned daemon did not become healthy: ${daemonOutput}`);
      }

      async function stopDaemon() {
        const child = daemon;
        if (
          !child ||
          !child.pid ||
          child.exitCode !== null ||
          child.signalCode !== null
        ) {
          daemon = undefined;
          return;
        }
        await new Promise<void>((resolveStop, reject) => {
          const force = setTimeout(() => {
            child.kill('SIGKILL');
          }, 5_000);
          const deadline = setTimeout(() => {
            clearTimeout(force);
            reject(new Error(`Owned daemon ${String(child.pid)} did not stop`));
          }, 10_000);
          child.once('exit', () => {
            clearTimeout(force);
            clearTimeout(deadline);
            resolveStop();
          });
          child.kill();
        });
        daemon = undefined;
      }

      async function cleanupSource() {
        if (!sourceDirectory || !sourceIdentity || !temporaryParent) return;
        const current = await lstat(sourceDirectory, { bigint: true });
        if (
          dirname(sourceDirectory) !== temporaryParent ||
          !basename(sourceDirectory).startsWith(sourcePrefix) ||
          !current.isDirectory() ||
          current.isSymbolicLink() ||
          (await realpath(sourceDirectory)) !== sourceDirectory ||
          current.dev !== sourceIdentity.dev ||
          current.ino !== sourceIdentity.ino
        )
          throw new Error(
            'Owned knowledge source directory was replaced or redirected',
          );
        await rm(sourceDirectory, {
          recursive: true,
          maxRetries: 3,
          retryDelay: 100,
        });
      }

      try {
        privateHome = await createPrivateTestHome('anima-knowledge-private-');
        temporaryParent = await realpath(tmpdir());
        // An ordinary non-hidden Temp folder is outside both the workspace and
        // private profile. Only the explicit owner grant makes it searchable.
        sourceDirectory = await mkdtemp(join(temporaryParent, sourcePrefix));
        sourceIdentity = await lstat(sourceDirectory, { bigint: true });
        const sourcePath = join(sourceDirectory, 'research.md');
        const needle = 'cobaltneedle';
        const replacement = 'silverneedle';
        const original = `${'Earlier unrelated project notes.\n'.repeat(1_024)}\n${needle}: the harbour opens at dawn.\n`;
        expect(
          Buffer.byteLength(original.slice(0, original.indexOf(needle))),
        ).toBeGreaterThan(16 * 1_024);
        await writeFile(sourcePath, original);
        await new Promise<void>((resolveListen, reject) => {
          model.once('error', reject);
          model.listen(0, '127.0.0.1', resolveListen);
        });
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
              listener.close(() => reject(new Error('No test daemon port')));
              return;
            }
            listener.close((error) =>
              error ? reject(error) : resolvePort(address.port),
            );
          });
        });
        client = createDaemonClient({
          baseUrl: `http://127.0.0.1:${port}`,
          ownerToken: DAEMON_TEST_OWNER_TOKEN,
          fetch: (input, init) => {
            const remaining = runtimeDeadline - Date.now();
            if (remaining <= 0)
              throw new Error('Knowledge process acceptance deadline exceeded');
            return fetch(input, {
              ...init,
              signal:
                init?.signal ??
                AbortSignal.timeout(Math.min(10_000, remaining)),
            });
          },
        });
        await startDaemon();
        await client.workspace.put({
          rootPath: privateHome.workspace,
          companyName: 'Knowledge test',
          mission: 'Isolated process acceptance',
          values: [],
        });
        const memberIds: string[] = [];
        for (const name of ['Knowledge researcher', 'Knowledge reviewer']) {
          const agent = await client.agents.create({
            name,
            model: 'knowledge-test',
            provider: 'openai',
            tools: [],
            settings: { autoTitle: false },
          });
          memberIds.push(agent.state.id);
        }
        const [researcher, reviewer] = memberIds;
        let project = await client.projects.create({
          requestKey: 'knowledge-process-project',
          title: 'Shared process evidence',
          memberAgentIds: memberIds,
        });
        project = await client.projects.addKnowledgeFolder(project.id, {
          expectedRevision: project.revision,
          requestKey: 'knowledge-process-folder',
          asAgentId: researcher,
          rootPath: sourceDirectory,
          label: 'Selected research folder',
        });
        expect(project.members.map((member) => member.agentId).sort()).toEqual(
          [...memberIds].sort(),
        );
        expect(project.resources).toHaveLength(1);
        const resource = project.resources[0];
        const grant = resource.target;
        expect(grant?.kind).toBe('knowledge_folder');
        if (grant?.kind !== 'knowledge_folder')
          throw new Error('Expected an explicit knowledge folder grant');
        expect(grant.rootIdentity).toEqual(expect.any(String));
        expect(grant.rootIdentity.length).toBeGreaterThan(0);
        // Windows canonical paths may carry the extended-length prefix. The
        // granted path must still name the exact directory selected above.
        const grantedIdentity = await lstat(grant.rootPath, { bigint: true });
        expect(grantedIdentity.isDirectory()).toBe(true);
        expect(grantedIdentity.isSymbolicLink()).toBe(false);
        expect({ dev: grantedIdentity.dev, ino: grantedIdentity.ino }).toEqual({
          dev: sourceIdentity.dev,
          ino: sourceIdentity.ino,
        });
        expect(resource.useInContext).toBe(true);

        let firstCitation:
          | Awaited<ReturnType<typeof client.projects.openKnowledgeCitation>>
          | undefined;
        for (const memberId of memberIds) {
          const found = await client.projects.searchKnowledge(
            project.id,
            memberId,
            { query: needle },
          );
          expect(found.sources).toEqual([
            expect.objectContaining({
              resourceId: resource.id,
              status: 'ready',
              documentCount: 1,
              omittedCount: 0,
            }),
          ]);
          expect(found.hits).toHaveLength(1);
          const citation = await client.projects.openKnowledgeCitation(
            project.id,
            found.hits[0].citationId,
            memberId,
          );
          expect(citation.resourceId).toBe(resource.id);
          expect(citation.text).toContain(
            `${needle}: the harbour opens at dawn.`,
          );
          const location = /^text#lines=\d+-\d+;byte=(\d+)$/.exec(
            citation.locator,
          );
          expect(location).not.toBeNull();
          const offset = Number(location![1]);
          expect(
            Buffer.from(original)
              .subarray(offset, offset + Buffer.byteLength(citation.text))
              .toString('utf8'),
          ).toBe(citation.text);
          firstCitation ??= citation;
        }
        const cachePath = join(
          privateHome.profile,
          'control-plane.json.project-knowledge',
          'knowledge.sqlite3',
        );
        expect((await stat(cachePath)).isFile()).toBe(true);
        expect(
          (await readFile(cachePath)).subarray(0, 16).toString('utf8'),
        ).toBe('SQLite format 3\0');

        await stopDaemon();
        await startDaemon();
        const restored = await client.projects.get(project.id, reviewer);
        expect(restored.members).toEqual(project.members);
        expect(restored.resources).toEqual(project.resources);
        // Opening the pre-restart ID before any new search demonstrates that
        // the persisted cache is usable with the restored grant and owner key.
        const reopened = await client.projects.openKnowledgeCitation(
          project.id,
          firstCitation!.citationId,
          reviewer,
        );
        expect(reopened.text).toBe(firstCitation!.text);
        expect(reopened.locator).toBe(firstCitation!.locator);
        const restoredSearch = await client.projects.searchKnowledge(
          project.id,
          reviewer,
          { query: needle },
        );
        expect(restoredSearch.hits).toHaveLength(1);
        const chat = await client.sessions.create(reviewer, {
          projectId: project.id,
          projectRevision: restored.revision,
          memberGeneration: restoredSearch.memberGeneration,
        });
        const firstRequest = requests.length;
        const run = await client.agents.run(
          reviewer,
          { text: needle },
          { roomId: chat.roomId },
        );
        expect(run.result.status, JSON.stringify(run.result)).toBe('success');
        const sent = requests
          .slice(firstRequest)
          .flatMap((request) => request.messages ?? [])
          .filter((message) => message.role === 'system');
        expect(sent.length).toBeGreaterThan(0);
        const citationUrl = `/api/projects/${project.id}/knowledge/citations/${restoredSearch.hits[0].citationId}?asAgentId=${reviewer}`;
        expect(
          sent.some(
            (message) =>
              typeof message.content === 'string' &&
              message.content.includes(citationUrl) &&
              message.content.includes(needle),
          ),
        ).toBe(true);
        expect(await readFile(sourcePath, 'utf8')).toBe(original);

        const beforeEdit = await stat(sourcePath);
        const changed = original.replace(needle, replacement);
        expect(Buffer.byteLength(changed)).toBe(Buffer.byteLength(original));
        await writeFile(sourcePath, changed);
        // Preserve size and modification time so freshness requires current
        // original-byte verification, rather than only metadata comparison.
        await utimes(sourcePath, beforeEdit.atime, beforeEdit.mtime);
        await expect(
          client.projects.openKnowledgeCitation(
            project.id,
            firstCitation!.citationId,
            researcher,
          ),
        ).rejects.toMatchObject({ status: 409 });
        expect(
          (
            await client.projects.searchKnowledge(project.id, researcher, {
              query: needle,
            })
          ).hits,
        ).toEqual([]);
        const updated = await client.projects.searchKnowledge(
          project.id,
          reviewer,
          { query: replacement },
        );
        expect(updated.hits).toHaveLength(1);
        expect(updated.hits[0].citationId).not.toBe(firstCitation!.citationId);
        const current = await client.projects.openKnowledgeCitation(
          project.id,
          updated.hits[0].citationId,
          reviewer,
        );
        expect(current.text).toContain(
          `${replacement}: the harbour opens at dawn.`,
        );
        expect(current.text).not.toContain(needle);
      } catch (error) {
        failure = error;
        // Vitest retains this diagnostic output after the owned files vanish.
        console.error(`Project knowledge process failure:\n${daemonOutput}`);
        throw error;
      } finally {
        const cleanupErrors: unknown[] = [];
        for (const cleanup of [
          stopDaemon,
          async () => {
            if (model.listening)
              await new Promise<void>((resolveClose, reject) =>
                model.close((error) =>
                  error ? reject(error) : resolveClose(),
                ),
              );
          },
          async () => {
            if (daemon)
              throw new Error(
                'Preserving private files because the owned daemon is still active',
              );
            await privateHome?.cleanup();
          },
          cleanupSource,
        ]) {
          try {
            await cleanup();
          } catch (error) {
            cleanupErrors.push(error);
          }
        }
        if (cleanupErrors.length) {
          console.error(
            `Project knowledge cleanup failure:\n${daemonOutput}`,
            cleanupErrors,
          );
          if (!failure)
            throw new AggregateError(
              cleanupErrors,
              'Project knowledge process cleanup failed',
            );
        }
      }
    }, 120_000);
  },
);
