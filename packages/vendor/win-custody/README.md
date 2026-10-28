# win-custody

Take custody of Windows child processes.

A service or launcher that starts helper processes on Windows has to decide,
for every child, what it inherits, how long it lives, who it runs as, and who
can talk to it. The Win32 defaults get each of these wrong in ways that are
easy to miss. `win-custody` wraps the calls that get them right behind safe,
owned types.

| You want the child to | Use |
|---|---|
| inherit exactly the handles you name, nothing else that is inheritable in your process | `Launch` (explicit `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`) |
| die with you, even when your process is killed | `Job` with `JobLimits::kill_on_close`, joined before the child's first instruction by `Launch::spawn_in_job` |
| run as the logged-on user, or as your service on that user's desktop | `PrimaryToken::for_session_user`, `PrimaryToken::duplicate_current_process` + `set_session_id`, then `Launch::as_user` |
| be reachable only by the principals you choose | `SecurityDescriptor::from_sddl` for `PipeServer`, `NamedMutex`, `Job` and directory trees |
| prove who is on the other end of a pipe | `PipeServer::client_process_id`, `PipeClient::server_process_id` |
| never block you forever | every pipe operation takes a `PipeWait` deadline and can end early when the peer exits |

```rust,no_run
use std::time::Duration;
use win_custody::{anonymous_pipe, read_to_end_bounded, Job, JobLimits, Launch};

fn main() -> std::io::Result<()> {
    // The job dies with this process, and takes the child with it.
    let job = Job::create(&JobLimits::new().kill_on_close(true).active_processes(4))?;

    let (output, output_writer) = anonymous_pipe()?;
    let child = Launch::new(r"C:\Windows\System32\cmd.exe")
        .args(["/d", "/c", "echo %GREETING%"])
        .env("GREETING", "hello from custody")
        .stdout(&output_writer)
        .spawn_in_job(&job)?;
    drop(output_writer);

    let text = read_to_end_bounded(&output, 64 * 1024)?;
    child.wait(Some(Duration::from_secs(10)))?;
    println!("{}", String::from_utf8_lossy(&text));
    Ok(())
}
```

Passing an extra handle to a child is one call; the return value is the
handle number the child will see:

```rust,no_run
use win_custody::{anonymous_pipe, Launch};

fn main() -> std::io::Result<()> {
    let (read_end, _write_end) = anonymous_pipe()?;
    let mut launch = Launch::new(r"C:\Program Files\Example\helper.exe");
    let value = launch.inherit(&read_end)?;
    launch.arg(format!("--input-handle={value}"));
    let _helper = launch.spawn()?;
    Ok(())
}
```

## Not a sandbox

Nothing here limits what a child can do with the rights its token already
has. A job object ties the child's lifetime and resource use to yours; it does
not stop the child from reading files, writing the registry or opening network
connections. To run code you do not trust, use an AppContainer or a separate
low-privilege account.

## How it relates to other crates

- [`std::process::Command`](https://doc.rust-lang.org/std/process/struct.Command.html)
  lets every inheritable handle in your process leak into the child. The
  explicit handle list (`spawn_with_attributes`, `inherit_handles`) is still
  unstable as of Rust 1.99.
- [`win32job`](https://crates.io/crates/win32job) and
  [`process-wrap`](https://crates.io/crates/process-wrap) manage job objects
  well. `win-custody` adds the suspended-create, assign, resume order that puts
  the child in the job before it runs, together with the handle list and
  tokens.
- [`interprocess`](https://crates.io/crates/interprocess) is the general
  cross-platform IPC crate. The pipes here are narrower: single-instance,
  local-only, first-instance-only, deadline-bound, with peer PID checks.
- [`winsafe`](https://crates.io/crates/winsafe) covers a large part of Win32
  safely, but not job objects, named pipe servers, SDDL, `CreateProcessAsUser`
  or process attribute lists.
- For services themselves, use
  [`windows-service`](https://crates.io/crates/windows-service); for the
  registry, [`windows-registry`](https://crates.io/crates/windows-registry).

## Where the `unsafe` lives

`unsafe_code` is denied for the crate; only the modules that call Win32 opt
back in. Every `unsafe` block carries a `SAFETY:` comment, Clippy's
`undocumented_unsafe_blocks` and `multiple_unsafe_ops_per_block` are errors,
and no raw handle, pointer or length crosses the public API: handles are
`std::os::windows::io::OwnedHandle` and `BorrowedHandle`, and buffers Windows
fills are bounds-checked before they are read.

The crate compiles to nothing on targets other than Windows, so it can sit in
the dependencies of a cross-platform project.

## Minimum supported Rust version

Rust 1.77.

## Origin

This code was first written for three applications by the same author, MacType
Control Center, Open Mobile Emulator and UAC Remote Controller, where it was
distributed under the GPL. The author, as its copyright holder, publishes it
here under the licenses below.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
