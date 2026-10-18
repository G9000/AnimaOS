"""Anima one-shot Python runner and native RPC protocol.

Status
======
The native broker, explicit Windows launcher and private transport are wired.
Synthetic physical acceptance covers native reads/writes, descendant timeout and
caller-loss cleanup. Interpreter configuration and native grants remain explicit;
this module's presence grants no production admission or successful outcome.
Persistent kernels, remote backends and bounded output spill remain separate work.

Reference and ownership
=======================
Public comparison: Hermes commit 1fd18277267152893a8a24792b79213e7ca3f899,
tools/code_execution_tool.py, code_execution_rpc.py, code_execution_env.py and
code_kernel.py. This protocol is independently designed for Anima's native
broker and schemas. It imports no Hermes code/config and copies no implementation.

The host owns interpreter selection, workspace identity, grants, policy, native
receipts, process custody and endpoint lifetime. Bootstrap data cannot select an
agent, session, run, receipt ID, filesystem root or additional tool grant. Python
is arbitrary Exec with same-user OS access; isolated startup is not an OS jail.

Launch and private bootstrap
============================
1. Require an explicitly selected existing absolute interpreter. No PATH, py
   launcher, Hermes installation, private profile, module install or fallback.
   Explicit selection of the bundled test interpreter never creates production
   configuration or admission.
2. Create a private owned staging directory, stage these fixed trusted bytes
   exclusively, and launch the interpreter directly with -I -S -u and the staged
   runner path. Only that fixed path and startup flags belong in argv. User code,
   descriptors, token and epoch never belong in argv or environment variables.
3. Clear environment; host allowlists only required OS/interpreter essentials
   and private TEMP/TMP. Do not inherit PYTHONPATH/PYTHONSTARTUP, provider or DB
   credentials, profiles, connector configuration or browser state.
4. Assign the child to the verified outer Windows Job before its user code can
   run. Read back required capabilities; fail closed if unavailable. Deadline
   and caller/run cancellation cover spawn, handshake and stdin delivery too.
5. The runner checks its supported Python version before emitting
   exactly b'ANIMA-TOOL-WORKER-v1\\n' on the original binary stdout and flushing.
   The host transmits no source/secret until that exact bounded READY succeeds.
6. The host writes one UTF-8 JSON object to stdin and closes stdin. The runner
   reads at most 524288 bytes plus one overflow sentinel, rejects any overflow,
   trailing non-whitespace, duplicate key, invalid UTF-8, non-finite number or
   unknown field. Required fields are version=1, code, reset, endpoint, token,
   cellEpoch, tools. The code is nonblank and at most 65536 UTF-8 bytes. reset
   is boolean; one_shot is always fresh, so reset never claims persistent state.
   endpoint is an exact host-generated 127.0.0.1 TCP port in 1..65535. Nonlocal
   addresses, URL strings, arbitrary hosts and fallback discovery are rejected.
   Credentials are bounded host-generated opaque strings, not owner identities.
   tools is at most nine bounded native descriptor objects selected by the host
   from the broker's captured intersection; no script can add an entry.
7. The native holder supplies code/reset and captured descriptors through the
   same bounded private bootstrap. The authoritative registry and current broker
   checks still determine which native calls can execute.

Native module and semantics
===========================
Install a synthetic stdlib ModuleType named anima_tools in sys.modules. Generate
functions only for received allowed native names. No eval of schema/doc strings
and no arbitrary setattr/import/module path supplied by bootstrap or the script.
Each generated function accepts keyword arguments only, serializes an object,
and calls the one client. Preserve descriptions and parameter schemas as bounded
untrusted documentation. The host still performs actual validation/authorization.

Initial fixed native family (then captured/current grant intersection):
    web_search, web_fetch, read_file, write_file, glob, grep, edit_file,
    multi_edit, bash.
No execute_code recursion, helper/delegation/messaging, automation, MCP, browser,
background/PTY/input, configuration or dynamic tool aliases. Native names and
argument schemas are authoritative; do not copy Hermes argument conventions:
read_file(file_path, offset=0, limit=...), write_file(file_path, content),
edit_file(file_path, old_string, new_string), multi_edit(file_path, edits),
glob(pattern, path=...), grep(pattern, path=..., include=...),
bash(command, timeout=...milliseconds, cwd=...workspace-relative).
Current native registry descriptors also supply web_search/web_fetch arguments.
The broker rejects any bash keys outside command, timeout and cwd.

Strict framed RPC
=================
Use one fresh loopback TCP connection per request with finite bounded I/O, no
automatic reconnect/retry. The host listener independently serializes calls and
enforces its own deadline; the Python lock alone establishes no authority.
Read/write exactly a four-byte unsigned NETWORK-endian length followed by that
many UTF-8 JSON bytes. Reject zero or length >65536 before allocating/reading the
body; use exact-read loops (short reads are normal, premature EOF is failure).
Reject extra response bytes/frames and close each connection after one exchange.
No readline-based unbounded buffer, pickle, repr/eval, remote request directory,
token in URL/query, or line-delimited implicit framing.

Request body is the broker's existing closed native envelope, not an upstream
Hermes payload:
    {"token": <private>, "request": {
        "version": 1, "cellEpoch": <private>, "sequence": <positive integer>,
        "tool": <exact native name>, "args": <JSON object>
    }}
The frame cap includes the envelope. Native core additionally validates depth16,
4096 nodes, 128 attempts including malformed/denied/replayed frames, and the
captured call cap: configured50 but at most49 after the fresh outer receipt and
less after earlier native actions. Code/descriptors do not expand these bounds.

Allocate monotonically increasing sequences under one threading lock. Serialize
the complete request and finish one exchange before another thread may dispatch.
No automatic retry after write/read timeout, EOF, invalid response, unknown
outcome or cancellation. A returned native failure consumed a dispatch sequence;
a protocol refusal does not justify guessing the next sequence and continuing.
Treat protocol/auth/order/capacity/uncertainty failures as terminal for the cell.
The native broker can replay an exact admitted request without another effect;
this does not authorize the client to repeat an uncertain write.

Response disposition is the closed set success, native_failed,
refused, review_required, with bounded native text and exact version/sequence at
the framing layer. A private typed native dispatch feeds this response; the
existing TaskResult projection remains for native callers/tests. Derive refused
from authentication/admission/live checks, native_failed from actual native tool
failure, and review_required only from journal uncertainty/pending native outcome.
Never infer disposition from error prefixes, Python/model text or guessed exit
codes. This is a projection of existing native outcomes, not another receipt
ledger. Do not serialize credentials or host owner/session/run scope.
Exact transport/target acceptance stays separate from fake-peer unit coverage.
The deadline closes admission and cancels without waiting for the broker mutex
or a pending native approval/storage operation; durable save futures still settle.

Execution and outcomes
======================
After validated bootstrap/module setup, compile the code with a fixed synthetic
filename and execute once in a fresh __main__ globals dictionary. Do not create a
persistent kernel, import user startup files, reuse previous globals, or advertise
reuse/execution history. Runner/API code can be inspected/changed by the script;
transport authentication never substitutes for the host's own grant checks.

Ordinary runtime/syntax exceptions are script failures with a nonzero leader exit
and bounded diagnostic output. SystemExit(0) remains only a script exit status;
os._exit, closing pipes or a printed result string never prove native call success
or physical retirement. Bootstrap/configuration errors remain separate from
script exceptions. Exit78 marks rejected bootstrap/configuration.

A native tool failure raises a distinct client exception. The script may catch
that exception and exit0; the host's RpcProgram summary still records failed or
refused native calls and must not report successful overall completion. Lost
native outcome persistence is effect uncertainty, never a verified tool result.
No child-created final summary is authoritative for native dispatches/receipts.

Output and custody
==================
Host-owned stdout/stderr readers capture raw bytes concurrently, never a Python
sys.stdout replacement as the only boundary (scripts can use os.write/children).
Capture refuses output beyond stdout50000/stderr10000. Explicit head/tail/total/
omitted counts and bounded owner-scoped spill are required before larger output
recovery is advertised. No newline is required to trigger a limit. Invalid UTF-8
must be handled explicitly without corrupting byte accounting. The host redacts
exact bootstrap secrets before logs/receipts/result previews; this is no general
secret-exfiltration or filesystem/network isolation claim.

On deadline, stop, caller drop, transport/framing failure or output overflow:
close native RPC admission first; signal the native cancellation/approval path;
terminate the whole owned Job; perform exactly one uncancelled whole-Job wait;
join both pipe tasks and all listener work. Preserve durable native save ordering
while the independent watchdog terminates Python. Storage and cleanup can settle
after the execution deadline, so do not claim a hard overall return-time bound.

Leader exit or pipe EOF alone cannot release capacity. Unknown spawn containment,
Job retirement, listener retirement or pipe join retains exact handles/directory/
permit in quarantine and reports cleanup uncertainty. No timeout+abort+detached
reaper may release a replacement slot. Default resource claims are only the
observed enforced capabilities: existing process-wrap JobObject supplies custody
and kill-on-close, not automatic CPU/memory/disk quotas or OS capability isolation.
Any additional required resource Job must be installed/read back before user code,
and descendants may not evade its declared process/memory limits.
"""

import json
import socket
import struct
import sys
import threading
import traceback
import types

_READY = b"ANIMA-TOOL-WORKER-v1\n"
_MAX_BOOTSTRAP = 524_288
_MAX_FRAME = 65_536
_TOOLS = frozenset(("web_search", "web_fetch", "read_file", "write_file", "glob",
                    "grep", "edit_file", "multi_edit", "bash"))


class NativeToolError(RuntimeError):
    """A native handler failed; catching this never clears host failure evidence."""


class RpcTransportError(RuntimeError):
    """No further RPC is admitted by this client after protocol/transport loss."""


class NativeOutcomeUncertain(RpcTransportError):
    """Effects may have occurred; the client must never retry the request."""


def _object(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError("duplicate JSON field")
        value[key] = item
    return value


def _nonfinite(_value):
    raise ValueError("non-finite JSON number")


def _decode(raw):
    return json.loads(raw.decode("utf-8"), object_pairs_hook=_object,
                      parse_constant=_nonfinite)


def _fields(value, expected):
    if not isinstance(value, dict) or set(value) != set(expected):
        raise ValueError("invalid closed protocol object")


def _text(value, limit):
    if not isinstance(value, str) or len(value.encode("utf-8")) > limit:
        raise ValueError("invalid bounded text")
    return value


def _exact(connection, count):
    result = bytearray()
    while len(result) < count:
        part = connection.recv(count - len(result))
        if not part:
            raise RpcTransportError("native RPC ended before its complete frame")
        result.extend(part)
    return bytes(result)


class _Client:
    def __init__(self, endpoint, token, epoch):
        self._endpoint = endpoint
        self._token = token
        self._epoch = epoch
        self._lock = threading.Lock()
        self._sequence = 1
        self._closed = False

    def call(self, name, arguments):
        with self._lock:
            if self._closed:
                raise RpcTransportError("this cell's RPC admission is closed")
            sequence = self._sequence
            try:
                body = json.dumps({"token": self._token, "request": {
                    "version": 1, "cellEpoch": self._epoch, "sequence": sequence,
                    "tool": name, "args": arguments}}, ensure_ascii=False,
                    allow_nan=False, separators=(",", ":")).encode("utf-8")
                if len(body) > _MAX_FRAME:
                    raise RpcTransportError("native RPC request exceeds frame limit")
                with socket.create_connection(self._endpoint, timeout=5) as connection:
                    # Approval waits count toward the independent host deadline;
                    # the client does not impose a shorter arbitrary approval cap.
                    connection.settimeout(300)
                    connection.sendall(struct.pack("!I", len(body)) + body)
                    # One exact request per connection; host refuses trailing
                    # bytes before dispatch rather than racing a socket peek.
                    connection.shutdown(socket.SHUT_WR)
                    length = struct.unpack("!I", _exact(connection, 4))[0]
                    if not 0 < length <= _MAX_FRAME:
                        raise RpcTransportError("invalid native RPC response length")
                    response = _decode(_exact(connection, length))
                    if connection.recv(1):
                        raise RpcTransportError("extra native RPC response frame")
                _fields(response, ("version", "sequence", "reply"))
                if type(response["version"]) is not int or response["version"] != 1:
                    raise ValueError("unsupported native response version")
                if type(response["sequence"]) is not int or response["sequence"] != sequence:
                    raise ValueError("wrong native response sequence")
                reply = response["reply"]
                _fields(reply, ("disposition", "text"))
                text = _text(reply["text"], _MAX_FRAME)
                disposition = reply["disposition"]
                if disposition not in ("success", "native_failed", "refused", "review_required"):
                    raise ValueError("unknown native response disposition")
            except Exception:
                self._closed = True
                # No bootstrap material, raw peer response or OS exception text
                # is included in the client transport error.
                raise RpcTransportError("native RPC transport/protocol failed; no retry") from None
            if disposition == "review_required":
                self._closed = True
                raise NativeOutcomeUncertain(text)
            if disposition == "refused":
                self._closed = True
                raise RpcTransportError(text)
            self._sequence += 1
            if disposition == "native_failed":
                raise NativeToolError(text)
            return text


def _module(descriptors, client):
    if not isinstance(descriptors, list) or len(descriptors) > len(_TOOLS):
        raise ValueError("invalid native descriptor list")
    module = types.ModuleType("anima_tools", "Captured native tool schemas; host checks remain authoritative.")
    seen = set()
    for descriptor in descriptors:
        _fields(descriptor, ("name", "description", "parametersSchema"))
        name = descriptor["name"]
        if not isinstance(name, str) or name not in _TOOLS or name in seen:
            raise ValueError("unavailable or duplicate native tool")
        description = _text(descriptor["description"], 8192)
        schema = descriptor["parametersSchema"]
        if not isinstance(schema, dict):
            raise ValueError("native parameter schema must be an object")
        schema_text = json.dumps(schema, allow_nan=False, ensure_ascii=False)
        _text(schema_text, 32768)
        seen.add(name)

        def bind(tool_name):
            def invoke(**kwargs):
                return client.call(tool_name, kwargs)
            return invoke

        function = bind(name)
        function.__name__ = name
        function.__doc__ = description + "\nNative keyword parameter schema: " + schema_text
        setattr(module, name, function)
    module.__all__ = sorted(seen)
    module.NativeToolError = NativeToolError
    module.RpcTransportError = RpcTransportError
    module.NativeOutcomeUncertain = NativeOutcomeUncertain
    return module


def _bootstrap(raw):
    value = _decode(raw)
    _fields(value, ("version", "code", "reset", "endpoint", "token", "cellEpoch", "tools"))
    if type(value["version"]) is not int or value["version"] != 1 or type(value["reset"]) is not bool:
        raise ValueError("invalid bootstrap version/reset")
    code = _text(value["code"], 65536)
    if not code.strip():
        raise ValueError("empty source")
    endpoint = value["endpoint"]
    _fields(endpoint, ("host", "port"))
    if endpoint["host"] != "127.0.0.1" or type(endpoint["port"]) is not int or not 0 < endpoint["port"] <= 65535:
        raise ValueError("invalid owned loopback endpoint")
    for key in ("token", "cellEpoch"):
        token = _text(value[key], 128)
        if not token.strip() or any(ord(character) < 32 or ord(character) == 127 for character in token):
            raise ValueError("invalid bootstrap credential")
    client = _Client((endpoint["host"], endpoint["port"]), value["token"], value["cellEpoch"])
    return code, _module(value["tools"], client)


def main():
    if sys.version_info < (3, 10):
        return 78
    sys.stdout.reconfigure(encoding="utf-8", errors="strict")
    sys.stderr.reconfigure(encoding="utf-8", errors="strict")
    sys.stdout.buffer.write(_READY)
    sys.stdout.buffer.flush()
    try:
        raw = sys.stdin.buffer.read(_MAX_BOOTSTRAP + 1)
        if len(raw) > _MAX_BOOTSTRAP:
            raise ValueError("bootstrap overflow")
        code, module = _bootstrap(raw)
    except Exception:
        print("Anima worker bootstrap refused", file=sys.stderr)
        return 78
    sys.modules["anima_tools"] = module
    try:
        compiled = compile(code, "<anima-one-shot>", "exec")
        exec(compiled, {"__name__": "__main__", "__builtins__": __builtins__})
    except SystemExit:
        raise
    except BaseException:
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
