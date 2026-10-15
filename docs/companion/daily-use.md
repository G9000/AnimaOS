# Anima daily-use setup

Anima is a Rust companion with persistent context and work that can run while the
console is closed. The selected feature set is implemented and fixture-tested.
Isolated local testing on 2026-10-12 also verified real ChatGPT replies, identity,
workspace file reading, memory recall, Stop, and persistence across daemon restart.
Messaging accounts, microphone and always-on deployment acceptance remain to be done.
Harness acceptance also verified a real multi-step report with a helper and saved
tasks, correction of existing memories, approved skill reuse, automatic context
compaction, and explicit continuation after an in-flight restart. Completed effects
were preserved; interrupted work was flagged for review before continuation.
Memory context now selects relevant own facts ahead of recent reflections, with
quoted, bounded excerpts. One real fresh-chat check recalled the corrected
preference after twenty newer reflections and eight matching foreign facts,
without using a search tool. Automatic context now keeps lexical matches first
and fills spare slots through the configured embedding backend. Learned recall
uses the existing optional FastEmbed backend; the default local backend remains
download-free. A real one-shot scheduled file task also
restored before due, fired without a manual trigger and retained one completed
write after another restart. Exact bytes and modification time stayed unchanged;
the schedule was left disabled and the isolated daemon stopped. A separate real
scheduled run was then crashed after a successful file write and saved native
action record, before its final conversation committed. Both subsequent restarts
retained the same action evidence and unchanged file; the interrupted schedule
stayed disabled and did not replay. Another one-time task remained enabled and
unclaimed while its due time passed with the daemon off; it ran automatically
after startup and remained completed without replay after a quiet restart. This
was a short local outage, not a test of recurring catch-up or hosted uptime.

The latest merged loop also passed one fresh memory-search/exact-edit/file task
and its tool-free final reply, followed by a quiet restart retaining the original
messages, action records and unchanged file. See the
[harness acceptance evidence](harness-acceptance.md) for tested behavior and gaps.

## Start with one companion

Use the existing [local workflow](../../AGENTS.md#dev-workflow) or
[single-owner VPS installation](../../deploy/vps/README.md#first-installation).
Those are operator instructions. Live acceptance used only an approved isolated
local web console and Rust daemon with synthetic data and no messaging connectors.
Choose a real configured model during onboarding, create the companion and select
its workspace. Confirm a real reply before relying on tools or schedules.

For durable local use, configure control-plane/history and memory persistence as
specified in the [daemon startup examples](../../hosts/rust-daemon/README.md#starting-the-daemon). An ephemeral preview is not a persistent assistant. Keep the
same workspace path and private credential vault across restart.

## Identity and everyday work

Place UTF-8 `SOUL.md` in the configured workspace root for the lead companion's
identity. Use `ANIMA.md` or `AGENTS.md` for project instructions. The console's
instruction preview shows what loaded; it does not edit those files. See
[standard Markdown files](../../hosts/rust-daemon/README.md#standard-markdown-agent-files)
for priority, limits and protected writes. File contents cannot override owner
permissions. Upstream profile/token files are never required.

Start a chat, give it a concrete task, and watch the tool cards and run status.
Use Stop when needed. Save a useful fact to memory; review and edit it in Memory.
With Collaborate or Operate access, the agent can correct its own visible memories
using their exact IDs and current content. Existing tool selections need an explicit
permission update to enable this new tool; saving unrelated settings does not add it.
For corrections, search for the complete current record before updating it; a
context excerpt is not the full text. Save compact durable facts and preferences,
use Tasks for temporary progress, and review reusable procedures as skills.
To let the companion remove an obsolete saved fact, explicitly enable **Allow
removing saved facts** in its settings. This grants the optional `memory_remove`
tool; it does not remove anything when enabled. Other custom permissions stay
intact when you toggle this option, and unrelated settings saves preserve it.
Switching to a standard access profile clears the option until you enable it again.
The agent must search for the exact current record first. It can remove only one
of its own visible Fact records, and a changed or partial match is refused.
Existing tool approval rules apply. This removes the active saved record;
old chats, reflections, derived historical knowledge, receipts and backups may
still retain its content. It is not complete erasure or automatic consolidation.
Console deletion stays tied to the complete text shown when you open its
confirmation. If the agent changes that text, deletion is refused and Memory
refreshes. Review the current record and explicitly open Delete again; the
console does not retry or fall back to deleting by ID alone.
To combine overlapping saved facts, explicitly enable **Allow consolidating saved
facts** in Settings. The companion must search for the full current text of
two to eight of its own facts, then propose a compact summary. One approved
operation saves that new Fact and moves the originals to historical Reflections,
preserving their original text, IDs and citations. A changed source refuses the
whole batch; search again before proposing another batch. General memory search
can still return those historical notes. This reduces active facts, not storage,
and the summary still needs judgment about accuracy. Exact duplicate default-scope
Fact saves reuse the existing record without changing its importance or tags;
paraphrases and case differences remain separate.

A bounded synthetic acceptance also exercised one model-authored consolidation
of two dragonfruit-allergy facts and one fresh Spanish recall after a quiet
same-store restart with no tools. See the
[harness evidence](harness-acceptance.md); this single example is not a general
accuracy guarantee.

Explicit search now filters eligible records before its lexical result limit, so
stronger foreign matches cannot hide your matching facts. Search and recent reads
exclude hidden text in record IDs, content and tags before limiting results;
corrections refuse those records too. Native vector recall now applies
canonical-record, supplied search-option and tool-visibility checks before
its top matches, so foreign, hidden or removed records still in the index cannot crowd out
your vector-only match. Automatic context now uses this filtered index once per
run, with a five-second result deadline and lexical fallback. Remote embeddings
may add one configured embedding request per run. Choose
`ANIMAOS_RS_MEMORY_EMBEDDINGS=fastembed` for learned multilingual recall; initial
model loading may download public weights. A fixed English/Spanish learned
test verifies ranking and actual fact injection. General semantic quality and
third-party index coverage remain unverified. The native local-index fixture passes;
a custom index using the legacy fallback can only filter its existing shortlist.

The native vector cache now checks the exact saved text before reusing a vector.
A stale vector cannot influence semantic ranking after a correction; the current
fact remains available through lexical recall while reindexing is pending.
Older cache rows without a text fingerprint rebuild once using the selected
backend. A configured remote backend can make embedding requests during this
rebuild. This changes the derived cache, not your saved facts or citations.

Use Files to inspect the chosen workspace. Attach text or an image when useful;
vision requires a compatible model. Selected DOCX and PDF attachments in your own
Windows web chat have bounded main-body or PDF text-layer extraction, with a
visible outcome and the original download retained. PDF coverage reports pages
with text and pages included, without inferring scanned content. See
[document coverage and limits](documents.md). OCR and other Office formats remain
storage/download only.

Selected WAV files can also be uploaded, then explicitly transcribed through the
opt-in OpenAI Platform API into an editable preview. Use transcript appends to
your current draft; Send stays separate. This requires a server API key, not
ChatGPT subscription sign-in. Stop is local; remote inference/billing may
continue. See [selected audio and limits](voice.md). Actual voice quality and
microphone/native speech output acceptance remain open.

Review tool permission requests in Approvals. Review saved mail drafts and
Calendar changes in Connectors before sending or applying them. Check the exact
request before allowing it. Configure a small schedule in Automations and inspect its
history. Schedules belong to the daemon, so they need that process running; a
closed browser is fine. An enabled, unclaimed one-time task can run after startup
if its due time passed while the daemon was off. Its original due time stays
visible after completion even though the task is disabled. A previously claimed,
interrupted action follows its recovery/review policy; inspect effects before retry.

For an interrupted, failed or stopped run, inspect its Recorded actions alongside
the actual files or external service. Each action records a bounded argument and
result preview. Reported success means the tool handler reported success; it does
not mean the whole run completed. Reported failure can still leave partial effects.
Outcome unknown means an intent was recorded without a settled result: the action
may have happened, or may never have started. Previews marked shortened are partial.

With a configured control-plane store, the daemon saves the action intent before
dispatch and the reported result before model continuation. A storage failure
stops continuation. Without that store, receipts are temporary and explicitly
marked memory only. Receipts do not commit an interrupted conversation or resume
an action. A crash between the effect and saving its result can still leave an
unknown outcome. Inspect the effect before an explicit retry; scheduled work is
not automatically replayed. Older runs without receipts cannot be backfilled.

Browser dictation/read-aloud depends on browser speech support and permissions.
An unavailable microphone or speech API does not prevent typed chat. Web fetch
and optional EXA search require explicit tool grants/configuration; they do not
provide interactive browser or desktop control. Preserve custom connector/web
permissions when changing access profiles: a standard profile replaces custom
tool selections, as the settings warning explains.

## When the tool budget runs out

The tool budget counts rounds, so a parallel batch uses one round. The default
is eight. After the limit, Anima gets one final model turn with tools disabled to
report confirmed results and unfinished work. A zero-round setting allows only
that text reply. Stop, provider errors and failed action-record saves still end
the run; an empty, truncated or rejected wrap-up does not retry. A completed
wrap-up means its reply was saved, not that every requested action succeeded.

Those saved replies show **Tool limit reached** beneath the message, including
older replies after reopening a chat. Review the tool results and remaining work;
the notice does not establish that the task succeeded. It leaves the saved reply,
copy/read-aloud text and memory-save action unchanged.

Accepted wrap-ups carry `budgetWrapup: true` in message metadata. The original
tool permissions remain available for the next independent run.

After three consecutive single calls fail with the same canonical arguments and
error, Anima gives the model one internal hint to reconsider the approach or
explain the blocker. The hint adds no turn or tool action and preserves the real
error records. Changed attempts, successful calls and parallel batches reset the
streak. It is advisory; it does not prove that no partial effects occurred.

Three identical fresh single-call plain-text successes can also produce one
factual hint per streak. Changed, structured, recovered and parallel results
reset it. Tools keep executing normally and their results remain intact;
identical output does not prove a lack of progress or effects. Necessary polling
is still allowed within the existing budget and Stop controls.

Anima can also notice a repeating sequence of two to four single-call results
after three complete repetitions. It gives at most one cycle hint per run and
leaves every call and result intact, including necessary polling. The default
eight-round limit can reach the two-call threshold; longer sequences need an
already-configured higher limit. The advice adds no request or budget extension
and does not prove that work stopped making progress.
If the model requests a tool that is not configured, one valid single request
can receive a paired refusal and a chance to correct its approach within the
same round budget. The refused request executes nothing and creates no action
receipt. A second refusal, malformed request, reused call ID or mixed unknown
batch ends the run. Permissions remain unchanged. Collapsed chat cards count
tool requests; inspect their status and Recorded actions for execution evidence.

ChatGPT subscription requests can retry once after a classified transient gateway
error, before streaming begins. The retry keeps the same request and account;
auth, quota, schema, ambiguous transport and partial-stream failures do not retry.
An explicit `max_retries: 0` disables transport retries. Final budget wrap-ups
disable them too, including existing API-provider stream retries. A failed
attempt may still consume usage; the limit does not guarantee a billing ceiling.

## Connect only what you use

- Telegram: configure and pair through Connectors. The native daemon handles
  polling, delivery and recovery; no separate gateway is needed.
- Discord: choose a text channel and permitted users, then explicitly activate.
  Read the [Discord setup and recovery limits](../../hosts/rust-daemon/CONNECTOR-CONTRIBUTING.md#selected-discord-channel).
  This is REST text polling, without Gateway voice or slash-command support.
- Gmail and Calendar: configure the Google OAuth app in Connectors → Google OAuth app, register the
  displayed callback URLs, then connect the account. Mail sends and Calendar
  writes follow their approval/recovery policies.
- Drive: enable the Google Drive API, paste 1–16 file IDs or recognized links and
  consent. [Selected Drive setup](../../hosts/rust-daemon/README.md#selected-drive-files)
  explains broader Google readonly authorization versus Anima's selected IDs,
  filename-only search and Google Docs/plaintext reads. Disconnect is local;
  account-wide Google revocation can also affect Gmail and Calendar.

Community connectors are reviewed source compiled into the daemon, with full
host privileges and a rebuild. [Contribution boundaries](../../hosts/rust-daemon/CONNECTOR-CONTRIBUTING.md)
are explicit; there is no dynamic module installer or sandbox claim.

## Acceptance before everyday reliance

These checks are **not executed** by the local fixture gates. Perform them when
real service/account testing is authorized, using your own selected accounts:

1. Receive a real streamed model response and stop another run. Confirm the
   conversation and status agree after reconnecting the console.
2. Save a memory and workspace file. Restart the daemon during a quiet period;
   confirm identity, conversation, memory, file and connector state remain.
3. Allow one low-impact external action and verify its actual result. Reject a
   second action and verify it did not happen. Never infer delivery from a tool
   card alone.
4. Send/receive a message through the selected Telegram/Discord channel. Inspect
   unknown send outcomes before retrying. Telegram is at-least-once; Discord/mail
   uncertainty is not an exactly-once guarantee.
5. Read an allowed Drive text file, search its name, and try an unselected ID.
   The last request must be refused. Check unsupported formats are explained.
6. Fire a small schedule and inspect its history. After an interrupted external
   attempt, inspect side effects before making another attempt.
7. For hosted use, follow the [VPS acceptance checklist](../../deploy/vps/README.md#acceptance-checks-before-relying-on-it),
   including reboot and a stopped complete backup/off-site restore rehearsal.

Long-running means durable identity, memory, schedules, progress and recovery.
It does not mean a model process runs forever or an interrupted network action
can resume halfway through. The daemon must remain available, credentials must
be valid, and the owner must review uncertain effects.
