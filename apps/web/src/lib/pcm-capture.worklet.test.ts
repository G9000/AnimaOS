import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { cwd } from 'node:process';
import { expect, it } from 'vitest';
import workletSource from './pcm-capture.worklet.js?raw';

type Complete = {
  version: number;
  type: 'complete';
  buffer: ArrayBuffer;
  usedBytes: number;
  frames: number;
  limitReached: boolean;
};
type Failure = { version: number; type: 'error'; reason: string };
type Reply = Complete | Failure;

class FakePort {
  onmessage: ((event: { data: unknown }) => void) | null = null;
  messages: Array<{ data: Reply; transfer?: ArrayBuffer[] }> = [];
  postMessage(data: Reply, transfer?: ArrayBuffer[]) {
    this.messages.push({ data, transfer });
  }
  send(data: unknown) {
    this.onmessage?.({ data });
  }
}

class FakeProcessorBase {
  port = new FakePort();
}

type Processor = FakeProcessorBase & {
  buffer: ArrayBuffer | null;
  view: DataView | null;
  process(inputs: Float32Array[][], outputs: Float32Array[][]): boolean;
};

function createProcessor(): Processor {
  // Evaluate the actual deployable script under fake AudioWorklet globals.
  const registration: { processorClass?: new () => Processor } = {};
  const evaluate = new Function(
    'AudioWorkletProcessor',
    'registerProcessor',
    workletSource,
  ) as (
    base: typeof FakeProcessorBase,
    register: (name: string, ctor: new () => Processor) => void,
  ) => void;
  evaluate(FakeProcessorBase, (name, ctor) => {
    expect(name).toBe('anima-pcm-capture-v1');
    registration.processorClass = ctor;
  });
  const processorClass = registration.processorClass;
  if (!processorClass) throw new Error('Worklet was not registered');
  return new processorClass();
}

function process(
  processor: Processor,
  samples: Float32Array,
  extraChannel?: Float32Array,
) {
  const output = new Float32Array([1, -1, 0.5]);
  expect(
    processor.process(
      [[samples, ...(extraChannel ? [extraChannel] : [])]],
      [[output]],
    ),
  ).toBe(true);
  expect(Array.from(output)).toEqual([0, 0, 0]);
}

function completion(processor: Processor): Complete {
  expect(processor.port.messages).toHaveLength(1);
  const message = processor.port.messages[0].data;
  expect(message.type).toBe('complete');
  if (message.type !== 'complete') throw new Error('Expected complete reply');
  expect(message.version).toBe(1);
  expect(processor.port.messages[0].transfer).toEqual([message.buffer]);
  expect(message.buffer.byteLength).toBe(3_840_044);
  expect(processor.buffer).toBeNull();
  expect(processor.view).toBeNull();
  return message;
}

it('matches the independent M56 4,000-frame PCM16 silence fixture byte for byte', () => {
  const processor = createProcessor();
  process(processor, new Float32Array(512));
  process(processor, new Float32Array(1_000));
  process(processor, new Float32Array(2_488));
  expect(processor.port.messages).toHaveLength(0);
  processor.port.send({ version: 1, type: 'stop' });
  const result = completion(processor);
  expect(result).toMatchObject({
    usedBytes: 8_044,
    frames: 4_000,
    limitReached: false,
  });
  const fixtureRelativePath =
    'hosts/rust-daemon/tests/fixtures/audio/m56-selected.wav';
  const workspaceFixture = resolve(cwd(), fixtureRelativePath);
  const fixture = readFileSync(
    existsSync(workspaceFixture)
      ? workspaceFixture
      : resolve(cwd(), '../../', fixtureRelativePath),
  );
  expect(Buffer.from(result.buffer, 0, result.usedBytes)).toEqual(fixture);
});

it('writes canonical little-endian WAV fields and signed clamped samples', () => {
  const processor = createProcessor();
  process(processor, new Float32Array([-2, -0.5, 0, 0.5, 2]));
  processor.port.send({ version: 1, type: 'stop' });
  const result = completion(processor);
  expect(result).toMatchObject({
    frames: 5,
    usedBytes: 54,
    limitReached: false,
  });
  const bytes = Buffer.from(result.buffer, 0, result.usedBytes);
  expect(bytes.toString('ascii', 0, 4)).toBe('RIFF');
  expect(bytes.readUInt32LE(4)).toBe(46);
  expect(bytes.toString('ascii', 8, 16)).toBe('WAVEfmt ');
  expect(bytes.readUInt32LE(16)).toBe(16);
  expect(bytes.readUInt16LE(20)).toBe(1);
  expect(bytes.readUInt16LE(22)).toBe(1);
  expect(bytes.readUInt32LE(24)).toBe(16_000);
  expect(bytes.readUInt32LE(28)).toBe(32_000);
  expect(bytes.readUInt16LE(32)).toBe(2);
  expect(bytes.readUInt16LE(34)).toBe(16);
  expect(bytes.toString('ascii', 36, 40)).toBe('data');
  expect(bytes.readUInt32LE(40)).toBe(10);
  expect(
    [0, 1, 2, 3, 4].map((index) => bytes.readInt16LE(44 + index * 2)),
  ).toEqual([-32_768, -16_384, 0, 16_384, 32_767]);
});

it('completes a valid candidate exactly at the 120-second frame cap', () => {
  const processor = createProcessor();
  process(processor, new Float32Array(1_919_999));
  expect(processor.port.messages).toHaveLength(0);
  process(processor, new Float32Array([0.25, Number.NaN, 0.5]));
  const result = completion(processor);
  expect(result).toMatchObject({
    frames: 1_920_000,
    usedBytes: 3_840_044,
    limitReached: true,
  });
  expect(new DataView(result.buffer).getInt16(result.usedBytes - 2, true)).toBe(
    8_192,
  );
  process(processor, new Float32Array([1]));
  processor.port.send({ version: 1, type: 'stop' });
  expect(processor.port.messages).toHaveLength(1);
});

it('ignores inactive input blocks before and after valid mono audio', () => {
  const processor = createProcessor();
  const inactiveOutput = new Float32Array([1, -1]);
  expect(processor.process([[]], [[inactiveOutput]])).toBe(true);
  expect(Array.from(inactiveOutput)).toEqual([0, 0]);
  expect(processor.port.messages).toHaveLength(0);

  process(processor, new Float32Array([0.5, -0.5]));
  inactiveOutput.fill(1);
  expect(processor.process([[]], [[inactiveOutput]])).toBe(true);
  expect(Array.from(inactiveOutput)).toEqual([0, 0]);
  expect(processor.port.messages).toHaveLength(0);

  processor.port.send({ version: 1, type: 'stop' });
  const result = completion(processor);
  expect(result).toMatchObject({
    frames: 2,
    usedBytes: 48,
    limitReached: false,
  });
  const view = new DataView(result.buffer);
  expect(view.getInt16(44, true)).toBe(16_384);
  expect(view.getInt16(46, true)).toBe(-16_384);
});

it('returns empty when stopped after only inactive input blocks', () => {
  const processor = createProcessor();
  const output = new Float32Array([1]);
  expect(processor.process([[]], [[output]])).toBe(true);
  expect(Array.from(output)).toEqual([0]);
  expect(processor.port.messages).toHaveLength(0);
  processor.port.send({ version: 1, type: 'stop' });
  expect(processor.port.messages).toEqual([
    {
      data: { version: 1, type: 'error', reason: 'empty' },
      transfer: undefined,
    },
  ]);
  expect(processor.buffer).toBeNull();
  expect(processor.view).toBeNull();
});

it('rejects malformed and nonfinite input with one terminal error', () => {
  for (const inputs of [
    [] as Float32Array[][],
    [[new Float32Array(0)]],
    [[new Int16Array(1) as unknown as Float32Array]],
    [[new Float32Array([0]), new Float32Array([0])]],
    [[new Float32Array([0, Number.POSITIVE_INFINITY])]],
    [[new Float32Array([Number.NaN])]],
  ]) {
    const processor = createProcessor();
    processor.process(inputs, [[new Float32Array(3)]]);
    expect(processor.port.messages).toEqual([
      {
        data: { version: 1, type: 'error', reason: 'invalid_audio' },
        transfer: undefined,
      },
    ]);
    expect(processor.buffer).toBeNull();
    expect(processor.view).toBeNull();
    processor.port.send({ version: 1, type: 'stop' });
    expect(processor.port.messages).toHaveLength(1);
  }
});

it('returns empty on an immediate stop or cancel, and rejects unknown commands', () => {
  const stopped = createProcessor();
  stopped.port.send({ version: 1, type: 'stop' });
  expect(stopped.port.messages[0].data).toEqual({
    version: 1,
    type: 'error',
    reason: 'empty',
  });

  const cancelled = createProcessor();
  process(cancelled, new Float32Array([1]));
  cancelled.port.send({ version: 1, type: 'cancel' });
  expect(cancelled.port.messages[0].data).toEqual({
    version: 1,
    type: 'error',
    reason: 'empty',
  });
  expect(cancelled.buffer).toBeNull();
  cancelled.port.send({ version: 1, type: 'stop' });
  expect(cancelled.port.messages).toHaveLength(1);

  const invalid = createProcessor();
  invalid.port.send({ version: 2, type: 'stop' });
  expect(invalid.port.messages[0].data).toEqual({
    version: 1,
    type: 'error',
    reason: 'invalid_audio',
  });
});
