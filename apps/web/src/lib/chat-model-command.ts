import type { ChatModelPreference, ChatReasoningEffort } from '@animaos/sdk';

const efforts = new Set(['none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max']);
/** The server still validates provider/model capabilities and connection state. */
export function parseChatModelCommand(argument: string): ChatModelPreference | null {
  const words = argument.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return null;
  if (words.length === 1 && ['inherit', 'default'].includes(words[0])) return { mode: 'inherit' };
  if ((words.length === 2 || words.length === 3) && !['inherit', 'default'].includes(words[0]) && /^[a-z][a-z0-9_-]*$/.test(words[0]) && words[1].length <= 200 && (!words[2] || efforts.has(words[2]))) {
    return { mode: 'override', provider: words[0], model: words[1], ...(words[2] ? { reasoningEffort: words[2] as ChatReasoningEffort } : {}) };
  }
  throw new Error('Use /model, /model inherit, or /model <provider> <model> [supported thinking effort].');
}
