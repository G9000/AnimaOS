import { execFile, spawn, type ChildProcess } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, readFile, stat, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { createServer as createNetServer } from 'node:net';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { beforeAll, describe, expect, it } from 'vitest';
import {
  createDaemonClient,
  DaemonHttpError,
  isTerminalRunStatus,
  type Approval,
  type DocumentEditReview,
  type OfficeInventoryPage,
  type PreviewEditContext,
  type Run,
} from '../src/index.js';
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
// A supplied artifact permits a process-only acceptance run after the managed
// native gate has built the current daemon and private_test_home_peer once.
const suppliedBinary = process.env.ANIMA_PREVIEW_EDIT_TEST_BINARY;
const binary = suppliedBinary
  ? resolve(suppliedBinary)
  : join(
      target,
      'debug',
      process.platform === 'win32' ? 'anima-daemon.exe' : 'anima-daemon',
    );
const diagnostics = join(
  repository,
  'packages/sdk/test-output/preview-edit-integration',
);

interface ModelMessage {
  role: string;
  content: unknown;
  tool_call_id?: string;
}
interface ModelRequest {
  messages: ModelMessage[];
  tools?: { function: { name: string } }[];
}
interface Proposal {
  proposal_id: string;
  review_sha256: string;
  file_path: string;
}
interface Publication {
  version: number;
  path: string;
  sha256: string;
  size_bytes: number;
  download_url: string;
  provenance: string;
}

const sha256 = (bytes: string | Uint8Array) =>
  createHash('sha256').update(bytes).digest('hex');

function toolResult<T>(messages: ModelMessage[], id: string): T {
  const message = messages.find(
    (entry) => entry.role === 'tool' && entry.tool_call_id === id,
  );
  if (!message || typeof message.content !== 'string')
    throw new Error(`Missing native result for ${id}`);
  return JSON.parse(message.content) as T;
}

describe.skipIf(process.env.ANIMA_SKIP_SERVICE_TESTS === '1')(
  'selected preview edits in a real daemon process',
  { concurrent: false },
  () => {
    beforeAll(async () => {
      await preparePrivateTestHome();
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
            timeout: 300_000,
            maxBuffer: 8 * 1024 * 1024,
          },
        );
      if (!(await stat(binary)).isFile())
        throw new Error('Preview edit test daemon binary is unavailable');
    }, 360_000);

    it('retains the exact selection across restart and retry, then publishes only its reviewed literal text after owner AllowOnce', async () => {
      let privateHome: PrivateTestHome | undefined;
      let daemon: ChildProcess | undefined;
      let daemonOutput = '';
      let failure: unknown;
      let modelFailure: unknown;
      let modelUrl = '';
      let port = 0;
      let context: PreviewEditContext | undefined;
      const runtimeDeadline = Date.now() + 100_000;
      const requests: ModelRequest[] = [];
      const filePath = 'selected.html';
      const selected = 'old &amp; title';
      const replacement = `Bright <b>title</b> & "quoted" 'text'`;
      const text =
        'Replace only the selected title text with the requested literal text.';
      const escaped =
        'Bright &lt;b&gt;title&lt;/b&gt; &amp; &quot;quoted&quot; &#39;text&#39;';
      const original =
        '<!doctype html><html><body><p id="keep">Keep é 中文</p>' +
        `<h1>Before ${selected} after</h1>` +
        `<p id="other">${selected} stays here</p>` +
        '<script>window.retained = "<old>";</script></body></html>';
      const start = Buffer.from(original).indexOf(Buffer.from(selected));
      const end = start + Buffer.byteLength(selected);
      const published = Buffer.concat([
        Buffer.from(original).subarray(0, start),
        Buffer.from(escaped),
        Buffer.from(original).subarray(end),
      ]);
      let client = createDaemonClient({ ownerToken: DAEMON_TEST_OWNER_TOKEN });

      function checkGenericWriteRefusal(turn: ModelMessage[]) {
        const denied = turn.find(
          (message) => message.tool_call_id === 'preview-generic-write',
        );
        expect(denied?.content).toEqual(
          expect.stringContaining(
            'A selected preview edit requires its exact native document review',
          ),
        );
        expect(context?.source.path).toBe(filePath);
      }

      function checkedInventory(turn: ModelMessage[]): OfficeInventoryPage {
        const inspected = toolResult<OfficeInventoryPage>(turn, 'preview-inspect');
        expect(inspected.source_sha256).toBe(sha256(original));
        expect(inspected.format).toBe('html');
        return inspected;
      }

      function checkedProposal(turn: ModelMessage[]): Proposal {
        const proposal = toolResult<Proposal>(turn, 'preview-propose');
        expect(proposal.proposal_id).toMatch(/^edit_/);
        expect(proposal.review_sha256).toMatch(/^[a-f0-9]{64}$/);
        expect(proposal.file_path).toBe(context?.source.path);
        return proposal;
      }

      const model = createServer(async (request, response) => {
        if (
          request.method !== 'POST' ||
          request.url !== '/v1/chat/completions'
        ) {
          response.writeHead(404).end();
          return;
        }
        try {
          let body = '';
          for await (const chunk of request) body += String(chunk);
          const input = JSON.parse(body) as ModelRequest;
          requests.push(input);
          let lastOwner = input.messages.length - 1;
          while (lastOwner >= 0 && input.messages[lastOwner].role !== 'user')
            lastOwner--;
          expect(input.messages[lastOwner]?.content).toBe(text);
          expect(context).toBeDefined();
          // Only parse actual system-message JSON data. Tool schema names or
          // selection text copied into a user prompt cannot satisfy this proof.
          const selectedSystemReferences = input.messages
            .filter((message) => message.role === 'system')
            .flatMap((message) =>
              typeof message.content === 'string'
                ? message.content.split('\n')
                : [],
            )
            .flatMap((line): unknown[] => {
              try {
                return [JSON.parse(line.trim())];
              } catch {
                return [];
              }
            });
          expect(selectedSystemReferences).toContainEqual(context);
          const turn = input.messages.slice(lastOwner + 1);
          const has = (id: string) =>
            turn.some(
              (message) =>
                message.role === 'tool' && message.tool_call_id === id,
            );
          let call: { id: string; name: string; args: unknown } | undefined;
          if (!has('preview-generic-write')) {
            call = {
              id: 'preview-generic-write',
              name: 'edit_file',
              args: {
                file_path: filePath,
                old_string: `Before ${selected} after`,
                new_string: 'UNREVIEWED GENERIC WRITE',
              },
            };
          } else if (!has('preview-inspect')) {
            checkGenericWriteRefusal(turn);
            call = {
              id: 'preview-inspect',
              name: 'inspect_office_document',
              args: { source: { kind: 'workspace', file_path: filePath } },
            };
          } else if (!has('preview-propose')) {
            const inspected = checkedInventory(turn);
            call = {
              id: 'preview-propose',
              name: 'propose_document_edit',
              args: {
                source: { kind: 'workspace', file_path: filePath },
                expected_sha256: inspected.source_sha256,
                file_path: filePath,
                expected_destination: {
                  state: 'present',
                  sha256: inspected.source_sha256,
                },
                operations: [
                  {
                    type: 'html_replace_text',
                    start_utf8: start,
                    end_utf8: end,
                    expected_text: selected,
                    new_text: replacement,
                  },
                ],
              },
            };
          } else if (!has('preview-apply')) {
            const proposal = checkedProposal(turn);
            call = {
              id: 'preview-apply',
              name: 'apply_document_edit',
              args: {
                proposal_id: proposal.proposal_id,
                review_sha256: proposal.review_sha256,
                file_path: filePath,
                expected_destination: {
                  state: 'present',
                  sha256: sha256(original),
                },
              },
            };
          }
          response.setHeader('content-type', 'application/json');
          response.end(
            JSON.stringify({
              choices: [
                {
                  message: call
                    ? {
                        role: 'assistant',
                        content: null,
                        tool_calls: [
                          {
                            id: call.id,
                            type: 'function',
                            function: {
                              name: call.name,
                              arguments: JSON.stringify(call.args),
                            },
                          },
                        ],
                      }
                    : {
                        role: 'assistant',
                        content: 'The reviewed selected text was published.',
                      },
                  finish_reason: call ? 'tool_calls' : 'stop',
                },
              ],
              usage: {
                prompt_tokens: 10,
                completion_tokens: 4,
                total_tokens: 14,
              },
            }),
          );
        } catch (error) {
          modelFailure = error;
          response.writeHead(500).end('Scripted model assertion failed');
        }
      });

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
            ANIMAOS_RS_KEYCHAIN_NAMESPACE: `preview-edit-process-test-${port}`,
            OPENAI_API_KEY: 'isolated-preview-edit-test-key',
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
            if (
              (
                await client.requestJson<{ status: string }>('/health', {
                  signal: AbortSignal.timeout(1_000),
                })
              ).status === 'ok'
            )
              return;
          } catch {
            /* startup */
          }
          await new Promise((resolveDelay) => setTimeout(resolveDelay, 100));
        }
        throw new Error(`Owned daemon did not become healthy: ${daemonOutput}`);
      }

      async function stopDaemon() {
        const child = daemon;
        if (!child || child.exitCode !== null || child.signalCode !== null) {
          daemon = undefined;
          return;
        }
        if (!child.pid) throw new Error('Owned daemon has no process identity');
        await new Promise<void>((resolveStop, reject) => {
          const force = setTimeout(() => child.kill('SIGKILL'), 5_000);
          const deadline = setTimeout(() => {
            clearTimeout(force);
            reject(new Error(`Owned daemon ${String(child.pid)} did not stop`));
          }, 10_000);
          child.once('exit', (code, signal) => {
            daemonOutput += `\nOwned daemon ${String(child.pid)} exited: ${String(code)} / ${String(signal)}\n`;
            clearTimeout(force);
            clearTimeout(deadline);
            resolveStop();
          });
          child.kill();
        });
        daemon = undefined;
      }

      async function pendingApproval(agentId: string, runId: string) {
        const deadline = Math.min(Date.now() + 25_000, runtimeDeadline);
        while (Date.now() < deadline) {
          if (modelFailure) throw modelFailure;
          const pending = await client.approvals.list({
            status: 'pending',
            agentId,
          });
          const approval = pending.approvals.find(
            (entry) => entry.runId === runId,
          );
          if (approval) return approval;
          const run = await client.runs.get(agentId, runId);
          if (isTerminalRunStatus(run.status))
            throw new Error(
              `Run ended before the exact review approval: ${JSON.stringify(run)}`,
            );
          await new Promise((resolveDelay) => setTimeout(resolveDelay, 50));
        }
        throw new Error('Selected preview edit did not request owner approval');
      }

      async function completedRun(agentId: string, runId: string): Promise<Run> {
        const deadline = Math.min(Date.now() + 20_000, runtimeDeadline);
        while (Date.now() < deadline) {
          if (modelFailure) throw modelFailure;
          const run = await client.runs.get(agentId, runId);
          if (isTerminalRunStatus(run.status)) return run;
          await new Promise((resolveDelay) => setTimeout(resolveDelay, 50));
        }
        throw new Error('Approved selected preview edit did not finish');
      }

      async function checkReview(
        approval: Approval,
        agentId: string,
        sessionId: string,
      ): Promise<DocumentEditReview> {
        expect(approval.tool).toBe('apply_document_edit');
        expect(approval.status).toBe('pending');
        expect(approval.argumentsTruncated).toBe(false);
        expect(approval.matcherKinds).toEqual(['any']);
        const bound = JSON.parse(approval.arguments);
        expect(bound).toMatchObject({
          _anima_exact_document_edit: true,
          source_sha256: sha256(original),
          candidate_sha256: sha256(published),
          file_path: filePath,
          expected_destination: { state: 'present', sha256: sha256(original) },
        });
        const review = await client.documentEdits.review(
          bound.proposal_id,
          agentId,
          sessionId,
        );
        expect(review.review_sha256).toBe(bound.review_sha256);
        expect(review.source_sha256).toBe(context!.source.sha256);
        expect(review.before.format).toBe('html');
        expect(review.after.format).toBe('html');
        expect(review.changes).toEqual([
          {
            operation_index: 0,
            label: 'Selected HTML text',
            before: selected,
            after: escaped,
            derived: false,
          },
        ]);
        expect(review.before.targets).toContainEqual(
          expect.objectContaining({
            value: {
              kind: 'html_text',
              start_utf8: start - Buffer.byteLength('Before '),
              end_utf8: end + Buffer.byteLength(' after'),
              text: `Before ${selected} after`,
            },
          }),
        );
        expect(review.after.targets).toContainEqual(
          expect.objectContaining({
            value: expect.objectContaining({
              kind: 'html_text',
              text: `Before ${escaped} after`,
            }),
          }),
        );
        expect(review.preservation.unchanged_bytes_verified).toBe(true);
        expect(await readFile(join(privateHome!.workspace, filePath), 'utf8')).toBe(
          original,
        );
        return review;
      }

      try {
        privateHome = await createPrivateTestHome('anima-preview-edit-private-');
        const sourcePath = join(privateHome.workspace, filePath);
        await writeFile(sourcePath, original);
        await new Promise<void>((resolveListen, reject) => {
          model.once('error', reject);
          model.listen(0, '127.0.0.1', resolveListen);
        });
        const modelAddress = model.address();
        if (!modelAddress || typeof modelAddress === 'string')
          throw new Error('No scripted model port');
        modelUrl = `http://127.0.0.1:${modelAddress.port}/v1`;
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
              throw new Error('Preview edit process deadline exceeded');
            return fetch(input, {
              ...init,
              signal:
                init?.signal ??
                AbortSignal.timeout(Math.min(10_000, remaining)),
            });
          },
        });
        await startDaemon();
        const configureWorkspace = (rootPath: string) =>
          client.workspace.put({
            rootPath,
            companyName: 'Selected preview process test',
            mission: 'Owned local process acceptance',
            values: [],
          });
        await configureWorkspace(privateHome.workspace);
        const agent = await client.agents.create({
          name: 'Selected preview editor',
          provider: 'openai',
          model: 'preview-edit-test',
          tools: [
            'edit_file',
            'inspect_office_document',
            'propose_document_edit',
            'apply_document_edit',
          ],
          settings: {
            autoTitle: false,
            maxRetries: 0,
            maxToolIterations: 8,
          },
        });
        const agentId = agent.state.id;
        // The generic writer is explicitly pregranted and write policy allows
        // it. The native selected-edit restriction must still reject its call.
        expect(agent.state.config.tools).toContainEqual(
          expect.objectContaining({ name: 'edit_file' }),
        );
        await client.approvals.setPolicy(agentId, {
          write: 'allow',
          exec: 'allow',
          network: 'allow',
          delegate: 'allow',
        });
        const session = await client.sessions.create(agentId);
        const source = await client.documentEdits.inspectWorkspace(
          filePath,
          agentId,
          session.id,
        );
        expect(source.source_sha256).toBe(sha256(original));
        expect(source.source_scope).toMatch(/^[a-f0-9]{64}$/);
        expect(start).toBeGreaterThan(original.indexOf(selected));
        context = {
          version: 1,
          source: {
            kind: 'workspace',
            path: filePath,
            sha256: source.source_sha256,
            scope: source.source_scope!,
          },
          selection: {
            kind: 'html_text',
            start_utf8: start,
            end_utf8: end,
            expected_text: selected,
          },
        };
        const input = { text, documentEditContext: context };

        const otherWorkspace = join(privateHome.home, 'other-workspace');
        await mkdir(otherWorkspace);
        await writeFile(join(otherWorkspace, filePath), original);
        await configureWorkspace(otherWorkspace);
        const otherSource = await client.documentEdits.inspectWorkspace(
          filePath,
          agentId,
          session.id,
        );
        expect(otherSource.source_sha256).toBe(source.source_sha256);
        expect(otherSource.source_scope).not.toBe(source.source_scope);
        await expect(
          client.runs.start(agentId, session.id, input, {
            idempotencyKey: 'preview-stale-workspace',
          }),
        ).rejects.toMatchObject({ status: 409 });
        expect(await readFile(join(otherWorkspace, filePath), 'utf8')).toBe(original);
        expect(requests).toHaveLength(0);
        await configureWorkspace(privateHome.workspace);

        const stale = original.replace(selected, 'new &amp; title');
        await writeFile(sourcePath, stale);
        await expect(
          client.runs.start(agentId, session.id, input, {
            idempotencyKey: 'preview-stale-bytes',
          }),
        ).rejects.toMatchObject({ status: 409 });
        expect(requests).toHaveLength(0);
        await writeFile(sourcePath, original);

        const first = await client.runs.start(agentId, session.id, input, {
          idempotencyKey: 'preview-before-restart',
        });
        expect(first.run.input.documentEditContext).toEqual(context);
        const firstApproval = await pendingApproval(agentId, first.run.id);
        const firstReview = await checkReview(firstApproval, agentId, session.id);
        const waiting = await client.runs.get(agentId, first.run.id);
        expect(waiting.status).toBe('awaiting_approval');
        expect(waiting.input.documentEditContext).toEqual(context);
        await expect(
          client.runs.start(agentId, session.id, { text }, {
            idempotencyKey: 'preview-before-restart',
          }),
        ).rejects.toMatchObject({ status: 409 });
        expect(
          (
            await client.runs.start(agentId, session.id, input, {
              idempotencyKey: 'preview-before-restart',
            })
          ).run.id,
        ).toBe(first.run.id);

        const callsBeforeRestart = requests.length;
        await stopDaemon();
        await startDaemon();
        const restored = await client.runs.get(agentId, first.run.id);
        expect(restored.status).toBe('interrupted');
        expect(restored.input.documentEditContext).toEqual(context);
        expect(restored.idempotencyKey).toBe('preview-before-restart');
        expect(await readFile(sourcePath, 'utf8')).toBe(original);
        expect(
          (await client.approvals.list({ status: 'pending', agentId })).approvals,
        ).toEqual([]);
        const decided = await client.approvals.list({ status: 'decided', agentId });
        expect(decided.approvals).toContainEqual(
          expect.objectContaining({
            id: firstApproval.id,
            status: 'expired',
            resolution: expect.objectContaining({
              resolvedBy: 'restart',
              decision: null,
            }),
          }),
        );
        await expect(
          client.documentEdits.review(firstReview.proposal_id, agentId, session.id),
        ).rejects.toMatchObject({ status: 400 });
        await client.sessions.get(agentId, session.id);
        expect(requests).toHaveLength(callsBeforeRestart);

        // A waiting turn has not committed a transcript exchange. Its durable
        // ledger input supplies an explicit fresh owner retry instead; the
        // interrupted candidate and old approval grant no execution authority.
        const retrySession = await client.sessions.create(agentId);
        expect(restored.input.text).toBe(text);
        expect(requests).toHaveLength(callsBeforeRestart);
        expect(await readFile(sourcePath, 'utf8')).toBe(original);
        const second = await client.runs.start(
          agentId,
          retrySession.id,
          {
            text: restored.input.text,
            documentEditContext: restored.input.documentEditContext!,
          },
          { idempotencyKey: 'preview-owner-retry' },
        );
        expect(second.run.id).not.toBe(first.run.id);
        const approval = await pendingApproval(agentId, second.run.id);
        const review = await checkReview(approval, agentId, retrySession.id);
        expect(review.proposal_id).not.toBe(firstReview.proposal_id);
        await expect(
          client.approvals.decide(approval.id, {
            decision: 'allow_always',
            revision: approval.revision,
          }),
        ).rejects.toMatchObject({ status: 400 });
        expect(await readFile(sourcePath, 'utf8')).toBe(original);
        const allowed = await client.approvals.decide(approval.id, {
          decision: 'allow_once',
          revision: approval.revision,
        });
        expect(allowed.resolution).toMatchObject({
          decision: 'allow_once',
          resolvedBy: 'owner',
          matcher: null,
          ruleId: null,
        });
        const finished = await completedRun(agentId, second.run.id);
        expect(finished.status, JSON.stringify(finished)).toBe('completed');
        expect(finished.input.documentEditContext).toEqual(context);
        const actual = await readFile(sourcePath);
        expect(actual).toEqual(published);
        expect(actual.toString('utf8')).toContain(
          `<p id="other">${selected} stays here</p>`,
        );
        expect(actual.toString('utf8')).toContain(
          '<script>window.retained = "<old>";</script>',
        );
        expect(actual.toString('utf8')).not.toContain('<b>title</b>');
        const finalRequest = requests.at(-1)!;
        const publication = toolResult<Publication>(
          finalRequest.messages,
          'preview-apply',
        );
        expect(publication).toMatchObject({
          version: 1,
          path: filePath,
          sha256: sha256(actual),
          size_bytes: actual.length,
          download_url: `/api/workspace/file/download?path=${filePath}`,
          provenance: 'imported_edit',
        });
        expect(finished.actionReceipts).toContainEqual(
          expect.objectContaining({
            name: 'apply_document_edit',
            status: 'succeeded',
            resultPreview: expect.stringContaining(publication.sha256),
          }),
        );
        expect(
          finished.actionReceipts?.some((receipt) => receipt.name === 'edit_file'),
        ).toBe(false);
        expect(
          Buffer.from(await (await client.workspace.downloadFile(filePath)).arrayBuffer()),
        ).toEqual(published);
        // A completed exchange supports the chat retry API. It returns the
        // original raw owner text and the same native restriction as a draft,
        // without silently renewing its source snapshot or starting a model.
        const callsBeforeDraft = requests.length;
        const draftDeadline = Math.min(Date.now() + 5_000, runtimeDeadline);
        let retryDraft:
          | Awaited<ReturnType<typeof client.sessions.revise>>
          | undefined;
        while (!retryDraft && Date.now() < draftDeadline) {
          try {
            retryDraft = await client.sessions.revise(agentId, retrySession.id, {
              mode: 'retry',
            });
          } catch (error) {
            // The ledger may finish just before the worker releases its room.
            // Retry only that precise custody race, for at most five seconds.
            if (
              !(error instanceof DaemonHttpError) ||
              error.status !== 409 ||
              error.message !== 'Wait for the reply before revising this chat'
            )
              throw error;
            await new Promise((resolveDelay) => setTimeout(resolveDelay, 50));
          }
        }
        if (!retryDraft)
          throw new Error('Completed preview edit room remained busy for 5 seconds');
        expect(retryDraft.retryPrompt).toBe(text);
        expect(retryDraft.retryDocumentEditContext).toEqual(context);
        expect(requests).toHaveLength(callsBeforeDraft);
        expect(await readFile(sourcePath)).toEqual(published);
        expect(modelFailure).toBeUndefined();
      } catch (error) {
        failure = error;
        await mkdir(diagnostics, { recursive: true });
        await writeFile(join(diagnostics, 'daemon.log'), daemonOutput);
        await writeFile(
          join(diagnostics, 'model-requests.json'),
          JSON.stringify(requests, null, 2),
        );
        console.error(`Preview edit process failure:\n${daemonOutput}`);
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
            // The shared fixture verifies the exact owned root and Windows
            // owner-only ACLs before removing this test's private directory.
            await privateHome?.cleanup();
          },
        ]) {
          try {
            await cleanup();
          } catch (error) {
            cleanupErrors.push(error);
          }
        }
        if (cleanupErrors.length) {
          await mkdir(diagnostics, { recursive: true });
          await writeFile(join(diagnostics, 'daemon.log'), daemonOutput);
          console.error('Preview edit process cleanup failure', cleanupErrors);
          if (!failure)
            failure = new AggregateError(
              cleanupErrors,
              'Preview edit process cleanup failed',
            );
        }
      }
      if (failure) throw failure;
    }, 130_000);
  },
);
