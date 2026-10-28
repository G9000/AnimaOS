# Changelog

All notable changes to this crate are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate
follows [Semantic Versioning](https://semver.org/).

## 0.1.1 - 2026-10-05

### Added

- `SECURITY.md`: report vulnerabilities through GitHub's private
  vulnerability reporting. The file now ships in the package.

## 0.1.0 - 2026-10-02

First release.

### Added

- `Launch`: creates a child with an explicit `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`,
  optional standard handles, a sorted Unicode environment block, a working
  directory, a desktop, and an optional primary token (`CreateProcessAsUserW`).
  `spawn_in_job` creates the child suspended, assigns it to a job, then resumes
  it. `inherit` returns the handle value the child will see.
- `SuspendedChild`: terminates the child when dropped without being resumed.
- `Job` and `JobLimits`: kill-on-close, active-process and commit limits, with
  optional security.
- `Process`: fixed rights sets, adoption of a `std::process::Child`, elevation
  through the shell's `runas` verb, PID-reuse-safe identity (creation time),
  exit code, image path, session.
- `PrimaryToken`: duplicate of the own token, `WTSQueryUserToken`, session
  moves.
- `SecurityDescriptor` (from SDDL, tree application), `ObjectSecurityDescriptor`
  with validated owner and DACL views, `OwnedSid`, `PrivilegeGuard`,
  `current_user_sid_string`, `current_token_is_member_of`.
- `PipeServer` and `PipeClient`: single-instance, local-only, overlapped byte
  pipes with deadlines, peer-exit cancellation and peer PID queries.
- `NamedMutex` with explicit security.
- Session helpers: `current_session_id`, `process_session_id`,
  `active_console_session_id`, `session_processes`.
- `wait`, `anonymous_pipe`, `null_device`, `read_to_end_bounded`.
