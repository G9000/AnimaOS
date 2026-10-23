import { describe, expect, it } from 'vitest';
import { parseChatModelCommand } from './chat-model-command';
describe('chat model command', () => {
  it('keeps opening, inheritance and explicit selection distinct', () => {
    expect(parseChatModelCommand('')).toBeNull();
    expect(parseChatModelCommand('inherit')).toEqual({ mode: 'inherit' });
    expect(parseChatModelCommand('default')).toEqual({ mode: 'inherit' });
    expect(parseChatModelCommand('chatgpt gpt-6-astra high')).toEqual({ mode: 'override', provider: 'chatgpt', model: 'gpt-6-astra', reasoningEffort: 'high' });
    expect(parseChatModelCommand('openai gpt-5.4-mini')).toEqual({ mode: 'override', provider: 'openai', model: 'gpt-5.4-mini' });
  });
  it('rejects incomplete or silently discarded arguments', () => {
    for (const text of ['openai', 'inherit model', 'openai model ultra', 'openai model high ignored']) expect(() => parseChatModelCommand(text)).toThrow(/Use \/model/);
  });
});
