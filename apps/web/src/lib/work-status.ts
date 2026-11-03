import type { ToolStep } from './transcript';

/** What a kind of tool call looks like from the outside, in plain words.
 *  The first pattern that matches the tool's name wins. */
const PHRASES: readonly [RegExp, string][] = [
  [/^(web_search|exa_search|web_search_exa)$/, 'Looking that up'],
  [/^(web_fetch|browser_)/, 'Reading a web page'],
  [/^(memory_(add|update|remove|store|write|remember|consolidate)|remember|learn)/, 'Remembering that'],
  [/^(memory_|recent_memories|core_memory)/, 'Checking what I remember'],
  [/^(search_conversations|read_conversation)/, 'Looking back through our chats'],
  [/^(write_file|edit_file|multi_edit|apply_document_edit|write_document|propose_document_edit)$/, 'Making the changes'],
  [/^(read_file|read_document|read_document_source|read|list_dir|glob|grep|read_tool_result|list_results)$/, 'Reading through it'],
  [/^calculate$/, 'Doing the math'],
  [/^(bash|execute_code|bg_)/, 'Running a few things'],
  [/^(spawn_helper|delegate_|wait_helper_runs|list_helper_runs)/, 'Getting some help'],
  [/^(calendar_list)/, 'Checking your calendar'],
  [/^(create_automation|trigger_automation|create_standing_intent|calendar_create)/, 'Setting that up'],
  [/^(send_email|mail_|send_message|send_discord|broadcast_message)/, 'Writing a message'],
  [/^generate_image$/, 'Making an image'],
  [/^(propose_plan|todo_)/, 'Planning'],
  [/^get_current_time$/, 'Checking the time'],
];

function phrase(step: ToolStep): string {
  if (step.helper) return `Asking ${step.helper.label}`;
  return (
    PHRASES.find(([pattern]) => pattern.test(step.name))?.[1] ?? 'Working on it'
  );
}

/** The one line Simple mode shows while a run works: what the call in
 *  progress is doing, or that the agent is thinking between calls. */
export function workStatus(steps: readonly ToolStep[]): string {
  for (let index = steps.length - 1; index >= 0; index -= 1) {
    if (steps[index].status === 'running') return `${phrase(steps[index])}…`;
  }
  return 'Thinking…';
}
