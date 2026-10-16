"""Bounded Windows acceptance ownership. Importing this module starts nothing."""
import ctypes as C
from ctypes import wintypes as W
import hashlib
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import threading
import time

LIMIT_LOG = 1024 * 1024
PORTS = (18752, 18753)
WAIT_OBJECT_0, WAIT_TIMEOUT = 0, 258
SIZE_T = C.c_size_t
ULONG_PTR = C.c_size_t


class GuardError(RuntimeError):
    def __init__(self, operation, *, win32_error=None, pid=None, creation=None, wait_state=None):
        super().__init__(operation)
        self.diagnostic = {"operation": str(operation)[:120], "win32Error": win32_error,
                           "pid": pid, "creationTime100ns": creation, "waitState": wait_state}


class SECURITY_ATTRIBUTES(C.Structure):
    _fields_ = [("nLength", W.DWORD), ("lpSecurityDescriptor", W.LPVOID), ("bInheritHandle", W.BOOL)]


class STARTUPINFO(C.Structure):
    _fields_ = [("cb", W.DWORD), ("lpReserved", W.LPWSTR), ("lpDesktop", W.LPWSTR),
                ("lpTitle", W.LPWSTR), ("dwX", W.DWORD), ("dwY", W.DWORD),
                ("dwXSize", W.DWORD), ("dwYSize", W.DWORD), ("dwXCountChars", W.DWORD),
                ("dwYCountChars", W.DWORD), ("dwFillAttribute", W.DWORD), ("dwFlags", W.DWORD),
                ("wShowWindow", W.WORD), ("cbReserved2", W.WORD), ("lpReserved2", C.POINTER(W.BYTE)),
                ("hStdInput", W.HANDLE), ("hStdOutput", W.HANDLE), ("hStdError", W.HANDLE)]


class PROCESS_INFORMATION(C.Structure):
    _fields_ = [("hProcess", W.HANDLE), ("hThread", W.HANDLE), ("dwProcessId", W.DWORD), ("dwThreadId", W.DWORD)]


class IO_COUNTERS(C.Structure):
    _fields_ = [(name, C.c_ulonglong) for name in ("ReadOperationCount", "WriteOperationCount", "OtherOperationCount", "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]


class BASIC_LIMIT(C.Structure):
    _fields_ = [("PerProcessUserTimeLimit", C.c_longlong), ("PerJobUserTimeLimit", C.c_longlong),
                ("LimitFlags", W.DWORD), ("MinimumWorkingSetSize", SIZE_T), ("MaximumWorkingSetSize", SIZE_T),
                ("ActiveProcessLimit", W.DWORD), ("Affinity", ULONG_PTR), ("PriorityClass", W.DWORD), ("SchedulingClass", W.DWORD)]


class EXTENDED_LIMIT(C.Structure):
    _fields_ = [("BasicLimitInformation", BASIC_LIMIT), ("IoInfo", IO_COUNTERS),
                ("ProcessMemoryLimit", SIZE_T), ("JobMemoryLimit", SIZE_T),
                ("PeakProcessMemoryUsed", SIZE_T), ("PeakJobMemoryUsed", SIZE_T)]


class BASIC_ACCOUNTING(C.Structure):
    _fields_ = [(name, C.c_longlong) for name in ("TotalUserTime", "TotalKernelTime", "ThisPeriodTotalUserTime", "ThisPeriodTotalKernelTime")] + [(name, W.DWORD) for name in ("TotalPageFaultCount", "TotalProcesses", "ActiveProcesses", "TotalTerminatedProcesses")]


def _api():
    if os.name != "nt":
        raise GuardError("Windows is required")
    k = C.WinDLL("kernel32", use_last_error=True)
    signatures = {
        "CreateJobObjectW": ([W.LPVOID, W.LPCWSTR], W.HANDLE),
        "SetInformationJobObject": ([W.HANDLE, C.c_int, W.LPVOID, W.DWORD], W.BOOL),
        "QueryInformationJobObject": ([W.HANDLE, C.c_int, W.LPVOID, W.DWORD, C.POINTER(W.DWORD)], W.BOOL),
        "AssignProcessToJobObject": ([W.HANDLE, W.HANDLE], W.BOOL),
        "IsProcessInJob": ([W.HANDLE, W.HANDLE, C.POINTER(W.BOOL)], W.BOOL),
        "CreateProcessW": ([W.LPCWSTR, W.LPWSTR, W.LPVOID, W.LPVOID, W.BOOL, W.DWORD, W.LPVOID, W.LPCWSTR, C.POINTER(STARTUPINFO), C.POINTER(PROCESS_INFORMATION)], W.BOOL),
        "ResumeThread": ([W.HANDLE], W.DWORD),
        "TerminateProcess": ([W.HANDLE, W.UINT], W.BOOL),
        "TerminateJobObject": ([W.HANDLE, W.UINT], W.BOOL),
        "WaitForSingleObject": ([W.HANDLE, W.DWORD], W.DWORD),
        "GetExitCodeProcess": ([W.HANDLE, C.POINTER(W.DWORD)], W.BOOL),
        "CloseHandle": ([W.HANDLE], W.BOOL),
        "OpenProcess": ([W.DWORD, W.BOOL, W.DWORD], W.HANDLE),
        "GetProcessTimes": ([W.HANDLE, C.POINTER(W.FILETIME), C.POINTER(W.FILETIME), C.POINTER(W.FILETIME), C.POINTER(W.FILETIME)], W.BOOL),
        "QueryFullProcessImageNameW": ([W.HANDLE, W.DWORD, W.LPWSTR, C.POINTER(W.DWORD)], W.BOOL),
        "CreatePipe": ([C.POINTER(W.HANDLE), C.POINTER(W.HANDLE), C.POINTER(SECURITY_ATTRIBUTES), W.DWORD], W.BOOL),
        "SetHandleInformation": ([W.HANDLE, W.DWORD, W.DWORD], W.BOOL),
        "CreateFileW": ([W.LPCWSTR, W.DWORD, W.DWORD, C.POINTER(SECURITY_ATTRIBUTES), W.DWORD, W.DWORD, W.HANDLE], W.HANDLE),
    }
    for name, (args, result) in signatures.items():
        getattr(k, name).argtypes, getattr(k, name).restype = args, result
    return k


def _ok(value, operation, **context):
    if not value:
        code = C.get_last_error()
        raise GuardError(operation, win32_error=code, **context)
    return value


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def write_once(path, value):
    """Publish complete JSON atomically without replacing existing evidence."""
    path = Path(path)
    temporary = path.with_name(path.name + ".publishing")
    with open(temporary, "x", encoding="utf-8", newline="\n") as stream:
        json.dump(value, stream, indent=2, sort_keys=True)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())
    try:
        os.link(temporary, path)
    finally:
        temporary.unlink()


def bounded_json(path, cap=1024 * 1024):
    with open(path, "rb") as stream:
        raw = stream.read(cap + 1)
    if len(raw) > cap:
        raise GuardError("JSON evidence exceeds bound")
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise GuardError("duplicate JSON key")
            result[key] = value
        return result
    return json.loads(raw, object_pairs_hook=unique)


def _identity(k, handle, pid, allow_retired_image=False):
    state = k.WaitForSingleObject(handle, 0)
    if state not in (WAIT_OBJECT_0, WAIT_TIMEOUT):
        code = C.get_last_error()
        raise GuardError("process wait", win32_error=code, pid=int(pid), wait_state=state)
    times = [W.FILETIME() for _ in range(4)]
    _ok(k.GetProcessTimes(handle, *(C.byref(t) for t in times)), "process times", pid=int(pid), wait_state=state)
    creation = (times[0].dwHighDateTime << 32) | times[0].dwLowDateTime
    image, size = C.create_unicode_buffer(32768), W.DWORD(32768)
    error = None
    if not k.QueryFullProcessImageNameW(handle, 0, image, C.byref(size)):
        error = C.get_last_error()  # Preserve before any subsequent native call.
        if allow_retired_image and error in (5, 31) and state == WAIT_TIMEOUT:
            # Observe retirement once on the same retained handle. Never retry
            # the image query or infer retirement from an image error alone.
            state = k.WaitForSingleObject(handle, 0)
            if state not in (WAIT_OBJECT_0, WAIT_TIMEOUT):
                wait_error = C.get_last_error() if state == 0xFFFFFFFF else None
                failure = GuardError("process post-image wait", win32_error=wait_error,
                                     pid=int(pid), creation=creation, wait_state=state)
                failure.diagnostic["originalImageError"] = error
                raise failure
        if not (allow_retired_image and state == WAIT_OBJECT_0 and error in (5, 31)):
            raise GuardError("process image", win32_error=error, pid=int(pid), creation=creation, wait_state=state)
    elif not image.value:
        raise GuardError("process image empty", pid=int(pid), creation=creation, wait_state=state)
    return {"pid": int(pid), "image": None if error else str(Path(image.value).resolve()),
            "creationTime100ns": creation, "imageQueryError": error,
            "retiredAtCapture": state == WAIT_OBJECT_0}


class OwnedProcess:
    def __init__(self, k, handle, pid, label, allow_retired_image=False):
        self.k, self.handle, self.pid, self.label = k, handle, int(pid), label
        self.identity = _identity(k, handle, pid, allow_retired_image=allow_retired_image)
        self.lastObservation = dict(self.identity)
        self._observed_image = self.identity["image"]

    def alive(self):
        state = self.k.WaitForSingleObject(self.handle, 0)
        if state not in (WAIT_OBJECT_0, WAIT_TIMEOUT):
            code = C.get_last_error()
            raise GuardError("process wait", win32_error=code, pid=self.pid, creation=self.identity["creationTime100ns"], wait_state=state)
        return state == WAIT_TIMEOUT

    def verify(self):
        observed = _identity(self.k, self.handle, self.pid, allow_retired_image=True)
        if (observed["pid"] != self.identity["pid"] or
                observed["creationTime100ns"] != self.identity["creationTime100ns"] or
                (self.identity["retiredAtCapture"] and not observed["retiredAtCapture"]) or
                (observed["image"] is not None and self._observed_image is not None and
                 os.path.normcase(observed["image"]) != os.path.normcase(self._observed_image))):
            raise GuardError("retained process identity changed", pid=self.pid, creation=observed["creationTime100ns"])
        self.lastObservation = observed
        if observed["image"] is not None:
            self._observed_image = observed["image"]

    def wait(self, seconds):
        state = self.k.WaitForSingleObject(self.handle, max(0, int(seconds * 1000)))
        if state not in (WAIT_OBJECT_0, WAIT_TIMEOUT):
            code = C.get_last_error()
            raise GuardError("process wait", win32_error=code, pid=self.pid, creation=self.identity["creationTime100ns"], wait_state=state)
        return state == WAIT_OBJECT_0

    def stop(self, seconds=5):
        self.verify()
        if self.alive():
            _ok(self.k.TerminateProcess(self.handle, 73), "owned process termination")
        if not self.wait(seconds):
            raise GuardError("owned process did not retire")

    def exit_code(self):
        code = W.DWORD()
        _ok(self.k.GetExitCodeProcess(self.handle, C.byref(code)), "exit code")
        return int(code.value)

    def close(self):
        if self.handle:
            _ok(self.k.CloseHandle(self.handle), "process handle close", pid=self.pid, creation=self.identity["creationTime100ns"])
            self.handle = None


class OwnedJob:
    def __init__(self, api=None):
        self.k = api if api is not None else _api()
        self.handle = _ok(self.k.CreateJobObjectW(None, None), "job creation")
        self.processes, self.logs = [], []
        self.discovery_gaps = []
        limits = EXTENDED_LIMIT()
        limits.BasicLimitInformation.LimitFlags = 0x2000 | 0x8  # kill-on-close, active-process cap
        limits.BasicLimitInformation.ActiveProcessLimit = 64
        try:
            _ok(self.k.SetInformationJobObject(self.handle, 9, C.byref(limits), C.sizeof(limits)), "job limits")
        except BaseException:
            self.k.CloseHandle(self.handle)
            self.handle = None
            raise

    def _pump(self, read_handle, path):
        import msvcrt
        fd = msvcrt.open_osfhandle(read_handle, os.O_RDONLY | os.O_BINARY)
        total, kept, failure = 0, 0, False
        try:
            with os.fdopen(fd, "rb", buffering=0) as source, open(path, "xb") as output:
                while True:
                    chunk = source.read(8192)
                    if not chunk:
                        break
                    total += len(chunk)
                    remaining = max(0, LIMIT_LOG - kept)
                    if remaining:
                        output.write(chunk[:remaining])
                        kept += min(len(chunk), remaining)
        except BaseException:
            failure = True
        self.logs.append({"file": str(path), "bytesObserved": total, "bytesRetained": kept,
                          "truncated": total > kept, "failed": failure})

    def launch(self, executable, args, cwd, env, log_path, label):
        if not self.handle:
            raise GuardError("job is closed")
        exe = Path(executable).resolve(strict=True)
        if not exe.is_file() or any("\0" in item for item in [str(exe), *args]):
            raise GuardError("invalid fixed executable arguments")
        if Path(log_path).exists():
            raise GuardError("refusing existing process log")
        security = SECURITY_ATTRIBUTES(C.sizeof(SECURITY_ATTRIBUTES), None, True)
        read_pipe, write_pipe = W.HANDLE(), W.HANDLE()
        _ok(self.k.CreatePipe(C.byref(read_pipe), C.byref(write_pipe), C.byref(security), 0), "log pipe")
        stdin = None
        info, process = STARTUPINFO(), PROCESS_INFORMATION()
        owner, thread = None, None
        try:
            _ok(self.k.SetHandleInformation(read_pipe, 1, 0), "pipe noninheritance")
            stdin = self.k.CreateFileW("NUL", 0x80000000, 3, C.byref(security), 3, 0x80, None)
            if stdin in (None, C.c_void_p(-1).value):
                raise GuardError("NUL handle unavailable")
            info.cb, info.dwFlags, info.wShowWindow = C.sizeof(info), 0x100 | 1, 0
            info.hStdInput, info.hStdOutput, info.hStdError = stdin, write_pipe, write_pipe
            command = C.create_unicode_buffer(subprocess.list2cmdline([str(exe), *args]))
            environment = C.create_unicode_buffer("\0".join(f"{k}={v}" for k, v in sorted(env.items(), key=lambda pair: pair[0].upper())) + "\0\0")
            _ok(self.k.CreateProcessW(str(exe), command, None, None, True,
                                     0x4 | 0x400 | 0x08000000, environment, str(Path(cwd).resolve()), C.byref(info), C.byref(process)), "suspended child creation")
            # The process has not executed any application instruction yet.
            _ok(self.k.AssignProcessToJobObject(self.handle, process.hProcess), "suspended child job assignment")
            owner = OwnedProcess(self.k, process.hProcess, process.dwProcessId, label)
            self.processes.append(owner)
            if os.path.normcase(owner.identity["image"]) != os.path.normcase(str(exe)):
                raise GuardError("child image differs from fixed executable")
            thread = threading.Thread(target=self._pump, args=(read_pipe.value, Path(log_path)), daemon=True)
            thread.start()
            owner.log_thread = thread
            read_pipe = W.HANDLE()  # pump owns the handle from here
            if self.k.ResumeThread(process.hThread) == 0xFFFFFFFF:
                code = C.get_last_error()
                raise GuardError("resume failed", win32_error=code, pid=int(process.dwProcessId))
            return owner
        except BaseException:
            if process.hProcess:
                self.k.TerminateProcess(process.hProcess, 74)
                self.k.WaitForSingleObject(process.hProcess, 5000)
                if owner is None:
                    self.k.CloseHandle(process.hProcess)
            raise
        finally:
            for handle in (process.hThread, write_pipe.value, stdin, read_pipe.value):
                if handle and handle != C.c_void_p(-1).value:
                    self.k.CloseHandle(handle)

    def _member_ids(self):
        """One bounded snapshot of the same retained Job's current members."""
        class IDS(C.Structure):
            _fields_ = [("assigned", W.DWORD), ("count", W.DWORD), ("ids", ULONG_PTR * 128)]
        ids = IDS()
        _ok(self.k.QueryInformationJobObject(self.handle, 3, C.byref(ids), C.sizeof(ids), None), "job member query")
        if ids.count > 128 or ids.assigned > 128:
            raise GuardError("job member bound exceeded")
        return list(ids.ids[:ids.count])

    def capture_descendants(self):
        """PID discovery is read-only; all retirement uses the retained job/handles."""
        if not self.handle:
            return
        for pid in self._member_ids():
            handle = self.k.OpenProcess(0x1000 | 0x100000, False, pid)
            if not handle:
                code = C.get_last_error()
                if code != 87:
                    raise GuardError("job member open", win32_error=code, pid=int(pid))
                try:
                    remaining = self._member_ids()  # Exactly one requery; never retry the open.
                    if pid in remaining:
                        raise GuardError("unopened job member remains present", win32_error=code, pid=int(pid))
                    if len(self.discovery_gaps) >= 128:
                        raise GuardError("discovery gap bound exceeded", win32_error=code, pid=int(pid))
                except GuardError as failure:
                    # A query failure keeps its own native error, not the open's.
                    failure.diagnostic["originalOpenError"] = code
                    failure.diagnostic["pid"] = int(pid)
                    raise
                self.discovery_gaps.append({"pid": int(pid), "openError": code,
                                            "absentOnRequery": True, "image": None,
                                            "creationTime100ns": None, "individualRetirement": "unknown",
                                            "coveredByJobDrain": False})
                continue
            inside = W.BOOL()
            try:
                _ok(self.k.IsProcessInJob(handle, self.handle, C.byref(inside)), "job membership")
                if not inside.value:
                    raise GuardError("discovered process is not a job member", pid=int(pid))
                owner = OwnedProcess(self.k, handle, pid, "descendant", allow_retired_image=True)
                duplicate = next((p for p in self.processes if p.pid == pid and
                                  p.identity["creationTime100ns"] == owner.identity["creationTime100ns"]), None)
                if duplicate is not None:
                    duplicate.verify()
                else:
                    if len(self.processes) >= 128:
                        raise GuardError("retained identity bound exceeded")
                    self.processes.append(owner)
                    handle = None
            finally:
                if handle:
                    self.k.CloseHandle(handle)

    def close(self, seconds=10):
        deadline = time.monotonic() + seconds
        errors, diagnostics, proof = [], [], []
        active, empty = None, False
        def failure(operation, exc=None):
            errors.append(operation)
            if isinstance(exc, GuardError):
                diagnostics.append(exc.diagnostic)
            elif exc is not None:
                diagnostics.append({"operation": operation, "kind": type(exc).__name__[:80]})
        if self.handle:
            try:
                self.capture_descendants()
            except BaseException as exc:
                failure("final descendant capture failed", exc)
            try:
                _ok(self.k.TerminateJobObject(self.handle, 73), "owned job termination")
            except BaseException as exc:
                failure("owned job termination failed", exc)
                # Failure cannot consume the wait budget before the fallback
                # kills the tree: known handles/logs still need bounded joins.
                try:
                    _ok(self.k.CloseHandle(self.handle), "job fallback close")
                    self.handle = None
                except BaseException as close_error:
                    failure("job fallback close failed", close_error)
        for process in self.processes:
            retired = False
            try:
                retired = process.wait(max(0, deadline - time.monotonic()))
                if retired:
                    process.verify()
            except BaseException as exc:
                failure("recorded identity verification failed", exc)
            proof.append({"label": process.label, **process.identity, "retired": retired,
                          "lastObservation": getattr(process, "lastObservation", process.identity)})
            if not retired:
                failure("recorded identity still active")
            thread = getattr(process, "log_thread", None)
            if thread:
                try:
                    thread.join(max(0, deadline - time.monotonic()))
                    if thread.is_alive():
                        failure("log pipe did not retire")
                except BaseException as exc:
                    failure("log pipe join failed", exc)
            try:
                process.close()
            except BaseException as exc:
                failure("process handle close failed", exc)
        if self.handle:
            try:
                while True:
                    accounting = BASIC_ACCOUNTING()
                    _ok(self.k.QueryInformationJobObject(self.handle, 1, C.byref(accounting), C.sizeof(accounting), None), "job active process query")
                    active = int(accounting.ActiveProcesses)
                    if active == 0:
                        empty = True
                        break
                    if time.monotonic() >= deadline:
                        raise GuardError("job active process deadline")
                    time.sleep(min(0.02, max(0, deadline - time.monotonic())))
            except BaseException as exc:
                failure("job empty proof failed", exc)
            finally:
                try:
                    _ok(self.k.CloseHandle(self.handle), "job handle close")  # Kill-on-close fallback.
                except BaseException as exc:
                    failure("job handle close failed", exc)
                self.handle = None
        # Only the final whole-Job outcome covers discovery gaps. In particular,
        # absence from a member snapshot never proves an individual's retirement.
        gaps_covered = empty and active == 0 and not errors and not any(
            log["failed"] or log["truncated"] for log in self.logs)
        gaps = [{**gap, "coveredByJobDrain": gaps_covered} for gap in self.discovery_gaps]
        return {"identities": proof, "logs": self.logs, "errors": errors, "diagnostics": diagnostics,
                "jobEmptyBeforeClose": empty, "activeProcessesBeforeClose": active,
                "discoveryGaps": gaps, "unresolvedDiscoveryGaps": sum(not gap["coveredByJobDrain"] for gap in gaps)}


def listeners():
    """Read both IP-family owner tables. Never open or terminate listener owners."""
    if os.name != "nt":
        raise GuardError("Windows listener ownership required")
    api = C.WinDLL("iphlpapi", use_last_error=True).GetExtendedTcpTable
    api.argtypes = [W.LPVOID, C.POINTER(W.DWORD), W.BOOL, W.ULONG, C.c_int, W.ULONG]
    api.restype = W.DWORD
    result = []
    for family, width, port_offset, pid_offset in [(2, 24, 8, 20), (23, 56, 20, 52)]:
        size = W.DWORD(0)
        code = api(None, C.byref(size), False, family, 3, 0)
        if code not in (0, 122) or size.value > 4 * 1024 * 1024:
            raise GuardError("TCP owner table unavailable")
        buffer = C.create_string_buffer(size.value)
        code = api(buffer, C.byref(size), False, family, 3, 0)
        if code != 0:
            raise GuardError("TCP owner table changed or unavailable")
        raw = buffer.raw
        count = struct.unpack_from("<I", raw)[0]
        if 4 + count * width > len(raw):
            raise GuardError("invalid TCP owner table")
        for index in range(count):
            row = raw[4 + index * width:4 + (index + 1) * width]
            port = socket.ntohs(struct.unpack_from("<I", row, port_offset)[0] & 0xFFFF)
            if port in PORTS:
                address = socket.inet_ntop(socket.AF_INET if family == 2 else socket.AF_INET6, row[4:8] if family == 2 else row[:16])
                result.append({"port": port, "address": address, "pid": struct.unpack_from("<I", row, pid_offset)[0]})
    return result


def require_free(ports=PORTS):
    occupied = [row for row in listeners() if row["port"] in ports]
    if occupied:
        raise GuardError("isolated port occupied; no interference authorized")
    for port in ports:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
            probe.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
            probe.bind(("127.0.0.1", port))


def require_listener(process, port):
    process.verify()
    if not process.alive():
        raise GuardError("owned listener process exited")
    rows = [row for row in listeners() if row["port"] == port]
    if rows != [{"port": port, "address": "127.0.0.1", "pid": process.pid}]:
        raise GuardError("listener owner or address does not match retained process")
    return {**process.identity, "port": port, "address": "127.0.0.1"}
