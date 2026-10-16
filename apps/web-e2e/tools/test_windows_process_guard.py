"""Deterministic tests for the M71 Windows process ownership guard.

No test in this module creates a native process, socket listener, or Win32 job.
"""
import ctypes as C
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


MODULE_PATH = Path(__file__).with_name("windows_process_guard.py")
SPEC = importlib.util.spec_from_file_location("m71_windows_process_guard", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("Could not load the Windows process guard")
guard = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(guard)


class BASIC_ACCOUNTING(C.Structure):
    _fields_ = [
        ("TotalUserTime", C.c_longlong),
        ("TotalKernelTime", C.c_longlong),
        ("ThisPeriodTotalUserTime", C.c_longlong),
        ("ThisPeriodTotalKernelTime", C.c_longlong),
        ("TotalPageFaultCount", guard.W.DWORD),
        ("TotalProcesses", guard.W.DWORD),
        ("ActiveProcesses", guard.W.DWORD),
        ("TotalTerminatedProcesses", guard.W.DWORD),
    ]


class FakeProcess:
    def __init__(self, events, label="recorded"):
        self.events = events
        self.label = label
        self.identity = {"pid": 41, "image": "C:/Anima/edge.exe", "creationTime100ns": 77}
        self.log_thread = None

    def wait(self, _seconds):
        self.events.append(("process-wait", self.label))
        return True

    def verify(self):
        self.events.append(("process-verify", self.label))

    def close(self):
        self.events.append(("close-process", self.label))


class FakeWinAPI:
    """Small pointer-aware fake for the exact Win32 calls used by this guard."""

    def __init__(self):
        self.events = []
        self.wait_state = guard.WAIT_TIMEOUT
        self.wait_states = {}
        self.open_wait_state = None
        self.wait_error_after = None
        self.time_error = None
        self.image_error = None
        self.image_path = "C:/Anima/edge.exe"
        self.created = (0, 77)
        self.job_members = []
        self.in_job = True
        self.job_active = 0
        self.query_error = None
        self.query_error_class = 1
        self.terminate_job_error = None
        self.opened = {}
        self.open_count = 0
        self.closed = []

    @staticmethod
    def _pointer(value, target):
        return C.cast(value, C.POINTER(target)).contents

    def WaitForSingleObject(self, handle, milliseconds):
        self.events.append(("wait", handle, milliseconds))
        if self.wait_error_after is not None:
            C.set_last_error(self.wait_error_after)
        return self.wait_states.get(handle, self.wait_state)

    def GetProcessTimes(self, handle, creation, exit_time, kernel, user):
        self.events.append(("times", handle))
        if self.time_error is not None:
            C.set_last_error(self.time_error)
            return 0
        created = self._pointer(creation, guard.W.FILETIME)
        created.dwHighDateTime, created.dwLowDateTime = self.created
        for target in (exit_time, kernel, user):
            item = self._pointer(target, guard.W.FILETIME)
            item.dwHighDateTime = item.dwLowDateTime = 0
        return 1

    def QueryFullProcessImageNameW(self, handle, _flags, output, size):
        self.events.append(("image", handle))
        if self.image_error is not None:
            C.set_last_error(self.image_error)
            return 0
        output.value = self.image_path
        self._pointer(size, guard.W.DWORD).value = len(self.image_path)
        return 1

    def CreateJobObjectW(self, _attributes, _name):
        self.events.append(("create-job",))
        return 900

    def SetInformationJobObject(self, handle, info_class, _value, _size):
        self.events.append(("set-job-info", handle, info_class))
        return 1

    def QueryInformationJobObject(self, handle, info_class, info, _size, _returned):
        self.events.append(("query-job", handle, info_class))
        if self.query_error is not None and info_class == self.query_error_class:
            C.set_last_error(self.query_error)
            return 0
        if info_class == 3:  # JOBOBJECT_BASIC_PROCESS_ID_LIST
            base = C.cast(info, C.POINTER(guard.W.DWORD))
            base[0] = len(self.job_members)
            base[1] = len(self.job_members)
            alignment = C.alignment(guard.ULONG_PTR)
            offset = (2 * C.sizeof(guard.W.DWORD) + alignment - 1) // alignment * alignment
            ids = C.cast(C.addressof(info._obj) + offset, C.POINTER(guard.ULONG_PTR))
            for index, pid in enumerate(self.job_members):
                ids[index] = pid
        elif info_class == 1:  # JOBOBJECT_BASIC_ACCOUNTING_INFORMATION
            self._pointer(info, BASIC_ACCOUNTING).ActiveProcesses = self.job_active
        return 1

    def IsProcessInJob(self, process, job, inside):
        self.events.append(("membership", process, job))
        self._pointer(inside, guard.W.BOOL).value = self.in_job
        return 1

    def OpenProcess(self, _access, _inherit, pid):
        self.events.append(("open-process", pid))
        self.open_count += 1
        handle = 1000 + self.open_count
        self.opened.setdefault(pid, []).append(handle)
        if self.open_wait_state is not None:
            self.wait_states[handle] = self.open_wait_state
        return handle

    def CloseHandle(self, handle):
        self.events.append(("close-handle", handle))
        self.closed.append(handle)
        C.set_last_error(87)
        return 1

    def TerminateJobObject(self, handle, exit_code):
        self.events.append(("terminate-job", handle, exit_code))
        if self.terminate_job_error is not None:
            C.set_last_error(self.terminate_job_error)
            return 0
        self.job_active = 0
        return 1

    def CreatePipe(self, read_handle, write_handle, _security, _size):
        self.events.append(("create-pipe",))
        self._pointer(read_handle, guard.W.HANDLE).value = 501
        self._pointer(write_handle, guard.W.HANDLE).value = 502
        return 1

    def SetHandleInformation(self, handle, _mask, _flags):
        self.events.append(("set-handle-info", handle))
        return 1

    def CreateFileW(self, *_args):
        self.events.append(("create-stdin",))
        return 503

    def CreateProcessW(self, _application, _command, _process_attrs, _thread_attrs,
                       _inherit, _flags, _env, _cwd, _startup, process_info):
        self.events.append(("create-process",))
        info = self._pointer(process_info, guard.PROCESS_INFORMATION)
        info.hProcess, info.hThread = 600, 601
        info.dwProcessId, info.dwThreadId = 42, 43
        self.job_active = 1
        return 1

    def AssignProcessToJobObject(self, job, process):
        self.events.append(("assign-process", job, process))
        return 1

    def TerminateProcess(self, process, code):
        self.events.append(("terminate-process", process, code))
        self.wait_state = guard.WAIT_OBJECT_0
        return 1

    def ResumeThread(self, thread):
        self.events.append(("resume-thread", thread))
        return 1


class GuardIdentityTests(unittest.TestCase):
    def test_retired_error31_image_query_requires_signaled_handle(self):
        api = FakeWinAPI()
        api.wait_state = guard.WAIT_OBJECT_0
        api.image_error = 31

        process = guard.OwnedProcess(
            api, 1041, 41, "retired-child", allow_retired_image=True
        )

        self.assertEqual(
            process.identity,
            {
                "pid": 41,
                "image": None,
                "creationTime100ns": 77,
                "imageQueryError": 31,
                "retiredAtCapture": True,
            },
        )
        self.assertLess(
            next(i for i, event in enumerate(api.events) if event[0] == "wait"),
            next(i for i, event in enumerate(api.events) if event[0] == "image"),
        )

    def test_successful_image_identity_records_same_shape_for_live_process(self):
        api = FakeWinAPI()
        process = guard.OwnedProcess(api, 1041, 41, "live-child")
        self.assertEqual(
            process.identity,
            {
                "pid": 41,
                "image": str(Path(api.image_path).resolve()),
                "creationTime100ns": 77,
                "imageQueryError": None,
                "retiredAtCapture": False,
            },
        )

    def test_live_error31_stays_fatal_and_wait_precedes_image_query(self):
        api = FakeWinAPI()
        api.wait_state = guard.WAIT_TIMEOUT
        api.image_error = 31
        with self.assertRaises(guard.GuardError):
            guard.OwnedProcess(api, 1041, 41, "live-child", allow_retired_image=True)
        events = [event[0] for event in api.events]
        self.assertLess(events.index("wait"), events.index("image"))

    def test_wait_failure_prevents_time_and_image_queries(self):
        api = FakeWinAPI()
        api.wait_state = 0xFFFFFFFF
        with self.assertRaises(guard.GuardError):
            guard.OwnedProcess(api, 1041, 41, "unverified-child")
        self.assertEqual([event[0] for event in api.events], ["wait"])

    def test_time_query_failure_stays_fatal_even_after_retirement(self):
        api = FakeWinAPI()
        api.wait_state = guard.WAIT_OBJECT_0
        api.time_error = 5
        with self.assertRaises(guard.GuardError):
            guard.OwnedProcess(api, 1041, 41, "retired-child", allow_retired_image=True)
        self.assertEqual([event[0] for event in api.events], ["wait", "times"])

    def test_other_retired_image_error_stays_fatal(self):
        api = FakeWinAPI()
        api.wait_state = guard.WAIT_OBJECT_0
        api.image_error = 5
        with self.assertRaises(guard.GuardError):
            guard.OwnedProcess(api, 1041, 41, "retired-child", allow_retired_image=True)

    def test_retained_creation_time_change_is_detected(self):
        api = FakeWinAPI()
        process = guard.OwnedProcess(api, 1041, 41, "retained-child")
        api.created = (0, 78)
        with self.assertRaises(guard.GuardError):
            process.verify()

    def test_retired_reinspection_retains_initial_pinned_image_as_last_observation(self):
        api = FakeWinAPI()
        process = guard.OwnedProcess(api, 1041, 41, "known-primary")
        api.wait_state = guard.WAIT_OBJECT_0
        api.image_error = 31

        process.verify()

        self.assertEqual(process.identity["image"], str(Path(api.image_path).resolve()))
        self.assertEqual(process.identity["creationTime100ns"], 77)
        self.assertEqual(
            process.lastObservation,
            {
                "pid": 41,
                "image": None,
                "creationTime100ns": 77,
                "imageQueryError": 31,
                "retiredAtCapture": True,
            },
        )

    def test_primary_launch_keeps_exact_executable_image_admission(self):
        api = FakeWinAPI()
        api.image_path = "C:/Untrusted/edge.exe"
        job = guard.OwnedJob(api=api)
        fixture_root = Path(__file__).resolve().parents[3] / ".superpowers/m71-unit-fixtures"
        fixture_root.mkdir(parents=True, exist_ok=True)
        self.assertEqual(fixture_root.resolve(strict=True), fixture_root)
        with tempfile.TemporaryDirectory(dir=fixture_root, prefix="pinned-") as directory:
            owned_directory = Path(directory).resolve(strict=True)
            self.assertEqual(owned_directory, Path(directory))
            self.assertTrue(owned_directory.is_relative_to(fixture_root))
            executable = owned_directory / "pinned.exe"
            executable.write_bytes(b"synthetic executable path only")
            with self.assertRaisesRegex(guard.GuardError, "child image differs"):
                job.launch(executable, [], directory, {}, Path(directory, "child.log"), "pinned")
        self.assertEqual(api.closed.count(600), 0)
        proof = job.close(seconds=1)
        self.assertEqual(api.closed.count(600), 1)
        self.assertTrue(proof["identities"][0]["retired"])


class GuardJobCaptureTests(unittest.TestCase):
    def make_job(self):
        api = FakeWinAPI()
        api.job_members = [41]
        api.wait_state = guard.WAIT_OBJECT_0
        api.image_error = 31
        return api, guard.OwnedJob(api=api)

    def test_capture_allows_error31_only_after_job_membership_and_signaled_wait(self):
        api, job = self.make_job()
        job.capture_descendants()
        self.assertEqual(len(job.processes), 1)
        self.assertEqual(job.processes[0].identity["image"], None)
        self.assertEqual(job.processes[0].identity["imageQueryError"], 31)
        names = [event[0] for event in api.events]
        self.assertLess(names.index("membership"), names.index("wait"))
        self.assertLess(names.index("wait"), names.index("image"))
        self.assertNotIn(api.opened[41][0], api.closed)

    def test_failed_capture_closes_caller_owned_process_handle_once(self):
        api, job = self.make_job()
        api.image_error = 5
        with self.assertRaises(guard.GuardError):
            job.capture_descendants()
        self.assertEqual(api.closed.count(api.opened[41][0]), 1)

    def test_capture_refuses_nonmember_and_closes_opened_handle_once(self):
        api, job = self.make_job()
        api.in_job = False
        with self.assertRaises(guard.GuardError):
            job.capture_descendants()
        self.assertEqual(api.closed.count(api.opened[41][0]), 1)

    def test_repeated_capture_may_reinspect_but_does_not_duplicate_retained_owner(self):
        api, job = self.make_job()
        job.capture_descendants()
        retained_handle = job.processes[0].handle
        job.capture_descendants()
        self.assertEqual(sum(event[0] == "open-process" for event in api.events), 2)
        self.assertEqual(len(job.processes), 1)
        duplicate_handle = api.opened[41][1]
        self.assertEqual(api.closed.count(duplicate_handle), 1)
        self.assertEqual(api.closed.count(retained_handle), 0)

    def test_pid_reuse_with_new_creation_time_gets_new_owned_identity(self):
        api, job = self.make_job()
        job.capture_descendants()
        original = job.processes[0]
        api.created = (0, 78)
        api.image_error = None  # New live incarnation supplies its actual image.
        api.wait_states[original.handle] = guard.WAIT_OBJECT_0
        api.open_wait_state = guard.WAIT_TIMEOUT

        job.capture_descendants()

        self.assertEqual(len(job.processes), 2)
        self.assertEqual(job.processes[0].identity["creationTime100ns"], 77)
        self.assertEqual(job.processes[1].identity["creationTime100ns"], 78)
        self.assertNotEqual(job.processes[0].handle, job.processes[1].handle)
        self.assertEqual(api.closed.count(original.handle), 0)


class GuardJobCleanupTests(unittest.TestCase):
    def test_nonzero_active_count_times_out_and_retains_fallback_failure(self):
        api = FakeWinAPI()
        job = guard.OwnedJob(api=api)

        def terminate_without_drain(handle, code):
            api.events.append(("terminate-job", handle, code))
            api.job_active = 1
            return 1

        api.TerminateJobObject = terminate_without_drain
        ticks = [0.0]

        def monotonic():
            ticks[0] += 0.01
            return ticks[0]

        with patch.object(guard.time, "monotonic", side_effect=monotonic):
            with patch.object(guard.time, "sleep", return_value=None):
                proof = job.close(seconds=0.025)

        self.assertFalse(proof["jobEmptyBeforeClose"])
        self.assertEqual(proof["activeProcessesBeforeClose"], 1)
        self.assertTrue(proof["errors"])
        self.assertEqual(api.closed.count(900), 1)

    def test_job_termination_and_zero_count_precede_job_close(self):
        api = FakeWinAPI()
        job = guard.OwnedJob(api=api)
        job.processes.append(FakeProcess(api.events))

        proof = job.close(seconds=1)

        events = api.events
        terminal = len(events)
        terminate = next((i for i, event in enumerate(events) if event[0] == "terminate-job"), terminal)
        process_close = next((i for i, event in enumerate(events) if event[0] == "close-process"), terminal)
        zero_check = next(
            (i for i, event in enumerate(events) if event[0] == "query-job" and event[2] == 1),
            terminal,
        )
        job_close = next(
            (i for i, event in enumerate(events) if event == ("close-handle", 900)),
            terminal,
        )
        self.assertLess(terminate, process_close)
        self.assertLess(process_close, zero_check)
        self.assertLess(zero_check, job_close)
        self.assertIs(proof.get("jobEmptyBeforeClose"), True)
        self.assertEqual(proof.get("activeProcessesBeforeClose"), 0)

    def test_active_count_query_failure_is_retained_and_job_closes_as_fallback(self):
        api = FakeWinAPI()
        job = guard.OwnedJob(api=api)
        api.query_error = 1460

        proof = job.close(seconds=1)

        self.assertTrue(proof["errors"])
        self.assertIn(("close-handle", 900), api.events)
        self.assertIs(proof.get("jobEmptyBeforeClose"), False)
        self.assertTrue(
            any(
                diagnostic.get("win32Error") == 1460
                for diagnostic in proof.get("diagnostics", [])
            ),
            "cleanup must retain the original query error after later calls",
        )

    def test_termination_failure_is_reported_and_job_handle_still_closes(self):
        api = FakeWinAPI()
        job = guard.OwnedJob(api=api)
        api.job_active = 1
        api.terminate_job_error = 5

        proof = job.close(seconds=0.05)

        self.assertTrue(proof["errors"])
        self.assertIn(("close-handle", 900), api.events)
        self.assertIs(proof.get("jobEmptyBeforeClose"), False)


if __name__ == "__main__":
    unittest.main()
