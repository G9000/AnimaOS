//! Named pipes whose every operation has a deadline and can end early when
//! the peer process exits.
#![allow(unsafe_code)]

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, OwnedHandle, RawHandle};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_IO_PENDING, ERROR_NOT_FOUND, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED,
    HANDLE, STATUS_PENDING, TRUE, WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{
    ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX,
    PIPE_ACCESS_INBOUND, PIPE_ACCESS_OUTBOUND, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    GetNamedPipeServerProcessId, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
};
use windows_sys::Win32::System::IO::{
    CancelIoEx, GetOverlappedResult, GetOverlappedResultEx, OVERLAPPED,
};

use crate::event::ManualResetEvent;
use crate::handle::{owned_from_creation, wait, WaitOutcome};
use crate::security::SecurityDescriptor;
use crate::wide::wide_null;

const LOCAL_PIPE_PREFIX: &str = r"\\.\pipe\";
const DEFAULT_BUFFER_BYTES: u32 = 64 * 1024;

static CANCELLED_OPERATIONS: AtomicU64 = AtomicU64::new(0);

/// How long a pipe operation may take, and whose exit ends it early.
#[derive(Clone, Copy, Debug)]
pub struct PipeWait<'a> {
    /// The operation fails with [`PipeError::TimedOut`] once this passes.
    pub deadline: Instant,
    /// Each kernel wait is at most this long; the peer and the deadline are
    /// checked between slices.
    pub poll: Duration,
    /// A process (or any waitable object) whose signal aborts the operation
    /// with [`PipeError::PeerExited`].
    pub peer: Option<BorrowedHandle<'a>>,
}

impl<'a> PipeWait<'a> {
    /// Waits until `deadline`, checking every 10 ms, with no peer.
    pub fn until(deadline: Instant) -> Self {
        Self {
            deadline,
            poll: Duration::from_millis(10),
            peer: None,
        }
    }

    /// Waits for at most `timeout` from now, checking every 10 ms, with no
    /// peer.
    pub fn within(timeout: Duration) -> Self {
        Self::until(Instant::now() + timeout)
    }

    /// Also ends the operation when `peer` is signaled, typically the
    /// process on the other end.
    #[must_use]
    pub fn with_peer<H: AsHandle + ?Sized>(mut self, peer: &'a H) -> Self {
        self.peer = Some(peer.as_handle());
        self
    }
}

/// A failure from a deadline-bound pipe operation.
#[derive(Debug)]
#[non_exhaustive]
pub enum PipeError {
    /// The peer was signaled before the operation completed.
    PeerExited,
    /// The deadline passed before the operation completed.
    TimedOut,
    /// Checking the peer failed.
    PeerCheck(io::Error),
    /// The operation itself failed with this Win32 error.
    Io(io::Error),
    /// A write transferred no bytes.
    NoProgress,
    /// Cancelling a pending operation failed after the operation was reaped.
    Cancel(io::Error),
}

impl fmt::Display for PipeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PeerExited => formatter.write_str("the peer process exited"),
            Self::TimedOut => formatter.write_str("the operation timed out"),
            Self::PeerCheck(error) => write!(formatter, "checking the peer failed: {error}"),
            Self::Io(error) => error.fmt(formatter),
            Self::NoProgress => formatter.write_str("the pipe accepted no bytes"),
            Self::Cancel(error) => {
                write!(
                    formatter,
                    "cancelling the pending operation failed: {error}"
                )
            }
        }
    }
}

impl std::error::Error for PipeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::PeerCheck(error) | Self::Io(error) | Self::Cancel(error) => Some(error),
            Self::PeerExited | Self::TimedOut | Self::NoProgress => None,
        }
    }
}

impl From<PipeError> for io::Error {
    fn from(error: PipeError) -> Self {
        match error {
            PipeError::Io(error) => error,
            PipeError::TimedOut => io::Error::new(io::ErrorKind::TimedOut, error),
            other => io::Error::other(other),
        }
    }
}

/// The direction data flows, seen from the server.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipeDirection {
    /// The client writes, the server reads.
    Inbound,
    /// The server writes, the client reads.
    Outbound,
    /// Both sides read and write.
    Duplex,
}

/// The access a [`PipeClient`] opens the pipe with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipeAccess {
    /// Read only.
    Read,
    /// Write only.
    Write,
    /// Read and write.
    ReadWrite,
}

/// A single-instance, local-only byte pipe server.
///
/// The pipe is created with `FILE_FLAG_FIRST_PIPE_INSTANCE`, so creation
/// fails instead of attaching to a pipe some other process already created
/// under the same name, and with `PIPE_REJECT_REMOTE_CLIENTS`. Its access is
/// whatever the [`SecurityDescriptor`] grants.
#[derive(Debug)]
pub struct PipeServer(OwnedHandle);

impl PipeServer {
    /// Creates `name` (which must start with `\\.\pipe\`) with 64 KiB
    /// buffers.
    pub fn create(
        name: &str,
        direction: PipeDirection,
        descriptor: &SecurityDescriptor,
    ) -> io::Result<Self> {
        Self::create_with_buffers(
            name,
            direction,
            descriptor,
            DEFAULT_BUFFER_BYTES,
            DEFAULT_BUFFER_BYTES,
        )
    }

    /// Creates `name` with the given outbound and inbound buffer sizes. The
    /// sizes are advisory; Windows may round them.
    pub fn create_with_buffers(
        name: &str,
        direction: PipeDirection,
        descriptor: &SecurityDescriptor,
        out_buffer_bytes: u32,
        in_buffer_bytes: u32,
    ) -> io::Result<Self> {
        let name = local_pipe_name(name)?;
        let attributes = descriptor.attributes(false);
        let access = match direction {
            PipeDirection::Inbound => PIPE_ACCESS_INBOUND,
            PipeDirection::Outbound => PIPE_ACCESS_OUTBOUND,
            PipeDirection::Duplex => PIPE_ACCESS_DUPLEX,
        };
        // SAFETY: `name` is NUL-terminated and the borrowed security attributes
        // and descriptor remain live until CreateNamedPipeW returns.
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                access | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                out_buffer_bytes,
                in_buffer_bytes,
                0,
                attributes.as_ptr(),
            )
        };
        Ok(Self(owned_from_creation(handle)?))
    }

    /// Waits for a client to connect.
    pub fn connect(&self, wait: &PipeWait<'_>) -> Result<(), PipeError> {
        let event = ManualResetEvent::new().map_err(PipeError::Io)?;
        let mut overlapped = new_overlapped(&event);
        // SAFETY: the server handle was opened for overlapped I/O; `overlapped`
        // and its event stay alive through synchronous completion or wait/reap.
        if unsafe { ConnectNamedPipe(self.raw(), &mut overlapped) } != 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        match win32_code(&error) {
            Some(ERROR_PIPE_CONNECTED) => Ok(()),
            Some(ERROR_IO_PENDING) => wait_for_overlapped(self.raw(), &overlapped, wait).map(drop),
            _ => Err(PipeError::Io(error)),
        }
    }

    /// Drops the connected client, so the next [`connect`](Self::connect)
    /// can accept another.
    pub fn disconnect(&self) -> io::Result<()> {
        // SAFETY: the handle is live; the call takes no pointers.
        if unsafe { DisconnectNamedPipe(self.raw()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// The process ID of the connected client, as the kernel recorded it.
    /// Compare it with the PID of the child you started before trusting what
    /// the client sends.
    pub fn client_process_id(&self) -> io::Result<u32> {
        let mut pid = 0;
        // SAFETY: the server owns a live named-pipe handle and `pid` is a local
        // output that remains valid for the call.
        if unsafe { GetNamedPipeClientProcessId(self.raw(), &mut pid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        nonzero_pid(pid, "client")
    }

    /// Writes all of `bytes`.
    pub fn write_all(&self, bytes: &[u8], wait: &PipeWait<'_>) -> Result<(), PipeError> {
        write_all_overlapped(self.raw(), bytes, wait)
    }

    /// Reads into `buffer`, returning the byte count.
    pub fn read(&self, buffer: &mut [u8], wait: &PipeWait<'_>) -> Result<usize, PipeError> {
        read_overlapped(self.raw(), buffer, wait)
    }

    fn raw(&self) -> HANDLE {
        self.0.as_raw_handle()
    }
}

impl AsHandle for PipeServer {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.0.as_handle()
    }
}

impl AsRawHandle for PipeServer {
    fn as_raw_handle(&self) -> RawHandle {
        self.raw()
    }
}

/// A client connection to a local named pipe.
///
/// The pipe is opened with `SECURITY_IDENTIFICATION`, so the server can learn
/// who the client is but cannot act as the client.
#[derive(Debug)]
pub struct PipeClient(File);

impl PipeClient {
    /// Opens `name` (which must start with `\\.\pipe\`), retrying every
    /// `poll` while the pipe does not exist yet or is busy, until `deadline`.
    pub fn open(
        name: &str,
        access: PipeAccess,
        deadline: Instant,
        poll: Duration,
    ) -> Result<Self, PipeError> {
        local_pipe_name(name).map_err(PipeError::Io)?;
        loop {
            let mut options = OpenOptions::new();
            match access {
                PipeAccess::Read => options.read(true),
                PipeAccess::Write => options.write(true),
                PipeAccess::ReadWrite => options.read(true).write(true),
            };
            options.custom_flags(
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            );
            match options.open(name) {
                Ok(file) => return Ok(Self(file)),
                Err(error)
                    if matches!(
                        win32_code(&error),
                        Some(ERROR_FILE_NOT_FOUND | ERROR_PIPE_BUSY)
                    ) =>
                {
                    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                        return Err(PipeError::TimedOut);
                    };
                    std::thread::sleep(remaining.min(poll));
                }
                Err(error) => return Err(PipeError::Io(error)),
            }
        }
    }

    /// The process ID of the server, as the kernel recorded it. Compare it
    /// with the process you expect before trusting what the server sends.
    pub fn server_process_id(&self) -> io::Result<u32> {
        let mut pid = 0;
        // SAFETY: the client owns a live named-pipe handle; `pid` is a local
        // out value.
        if unsafe { GetNamedPipeServerProcessId(self.0.as_raw_handle(), &mut pid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        nonzero_pid(pid, "server")
    }

    /// Reads into `buffer`, returning the byte count.
    pub fn read(&self, buffer: &mut [u8], wait: &PipeWait<'_>) -> Result<usize, PipeError> {
        read_overlapped(self.0.as_raw_handle(), buffer, wait)
    }

    /// Writes all of `bytes`.
    pub fn write_all(&self, bytes: &[u8], wait: &PipeWait<'_>) -> Result<(), PipeError> {
        write_all_overlapped(self.0.as_raw_handle(), bytes, wait)
    }
}

impl AsHandle for PipeClient {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.0.as_handle()
    }
}

impl AsRawHandle for PipeClient {
    fn as_raw_handle(&self) -> RawHandle {
        self.0.as_raw_handle()
    }
}

fn local_pipe_name(name: &str) -> io::Result<Vec<u16>> {
    let is_local = name
        .get(..LOCAL_PIPE_PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(LOCAL_PIPE_PREFIX));
    if !is_local || name.len() == LOCAL_PIPE_PREFIX.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            r"pipe names must start with \\.\pipe\ and name a pipe",
        ));
    }
    wide_null(name)
}

fn nonzero_pid(pid: u32, side: &str) -> io::Result<u32> {
    if pid == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("the named pipe {side} reported process ID zero"),
        ));
    }
    Ok(pid)
}

fn new_overlapped(event: &ManualResetEvent) -> OVERLAPPED {
    OVERLAPPED {
        hEvent: event.as_raw(),
        ..Default::default()
    }
}

fn read_overlapped(
    handle: HANDLE,
    buffer: &mut [u8],
    wait: &PipeWait<'_>,
) -> Result<usize, PipeError> {
    let event = ManualResetEvent::new().map_err(PipeError::Io)?;
    let mut overlapped = new_overlapped(&event);
    let request = buffer.len().min(u32::MAX as usize) as u32;
    // SAFETY: `handle` is a live pipe opened for overlapped I/O; `buffer` is
    // writable for `request` bytes; the OVERLAPPED and its event stay alive
    // through synchronous completion or wait/reap. The byte-count pointer is
    // null as required for asynchronous handles.
    let completed = unsafe {
        ReadFile(
            handle,
            buffer.as_mut_ptr(),
            request,
            null_mut(),
            &mut overlapped,
        )
    };
    let read = if completed != 0 {
        completed_overlapped_result(handle, &overlapped)?
    } else {
        let error = io::Error::last_os_error();
        if win32_code(&error) != Some(ERROR_IO_PENDING) {
            return Err(PipeError::Io(error));
        }
        wait_for_overlapped(handle, &overlapped, wait)?
    };
    Ok((read as usize).min(request as usize))
}

fn write_all_overlapped(
    handle: HANDLE,
    mut bytes: &[u8],
    wait: &PipeWait<'_>,
) -> Result<(), PipeError> {
    while !bytes.is_empty() {
        let request = bytes.len().min(u32::MAX as usize);
        let event = ManualResetEvent::new().map_err(PipeError::Io)?;
        let mut overlapped = new_overlapped(&event);
        // SAFETY: `handle` is a live pipe opened for overlapped I/O; `bytes` is
        // readable for `request` bytes; the OVERLAPPED and its event stay alive
        // through synchronous completion or wait/reap. The byte-count pointer
        // is null as required for asynchronous handles.
        let completed = unsafe {
            WriteFile(
                handle,
                bytes.as_ptr(),
                request as u32,
                null_mut(),
                &mut overlapped,
            )
        };
        let written = if completed != 0 {
            completed_overlapped_result(handle, &overlapped)?
        } else {
            let error = io::Error::last_os_error();
            if win32_code(&error) != Some(ERROR_IO_PENDING) {
                return Err(PipeError::Io(error));
            }
            wait_for_overlapped(handle, &overlapped, wait)?
        };
        if written == 0 {
            return Err(PipeError::NoProgress);
        }
        bytes = &bytes[(written as usize).min(request)..];
    }
    Ok(())
}

fn completed_overlapped_result(handle: HANDLE, overlapped: &OVERLAPPED) -> Result<u32, PipeError> {
    let mut transferred = 0;
    // SAFETY: the operation reported synchronous completion; `handle` and its
    // unique OVERLAPPED remain live while the completed byte count is queried.
    if unsafe { GetOverlappedResult(handle, overlapped, &mut transferred, 0) } == 0 {
        return Err(PipeError::Io(io::Error::last_os_error()));
    }
    Ok(transferred)
}

fn wait_for_overlapped(
    handle: HANDLE,
    overlapped: &OVERLAPPED,
    pipe_wait: &PipeWait<'_>,
) -> Result<u32, PipeError> {
    loop {
        if let Some(peer) = pipe_wait.peer {
            match wait(&peer, Some(Duration::ZERO)) {
                Ok(WaitOutcome::Signaled) => {
                    return cancel_and_reap(handle, overlapped, PipeError::PeerExited);
                }
                Ok(WaitOutcome::TimedOut) => {}
                Ok(WaitOutcome::Abandoned) => {
                    let error = io::Error::other("the peer wait was abandoned");
                    return cancel_and_reap(handle, overlapped, PipeError::PeerCheck(error));
                }
                Err(error) => {
                    return cancel_and_reap(handle, overlapped, PipeError::PeerCheck(error));
                }
            }
        }
        let Some(remaining) = pipe_wait.deadline.checked_duration_since(Instant::now()) else {
            return cancel_and_reap(handle, overlapped, PipeError::TimedOut);
        };
        let slice_milliseconds = remaining
            .min(pipe_wait.poll)
            .as_millis()
            .clamp(1, u128::from(u32::MAX - 1)) as u32;
        let mut transferred = 0;
        // SAFETY: `handle` is live; `overlapped` and its event remain live until
        // this operation completes or is cancelled and reaped; `transferred` is
        // a local output.
        let finished = unsafe {
            GetOverlappedResultEx(handle, overlapped, &mut transferred, slice_milliseconds, 0)
        };
        if finished != 0 {
            return Ok(transferred);
        }
        let error = io::Error::last_os_error();
        if win32_code(&error) == Some(WAIT_TIMEOUT) {
            continue;
        }
        if overlapped_has_completed(overlapped) {
            // The operation finished with this error (a broken pipe, an
            // abort); there is nothing left to cancel or reap.
            return Err(PipeError::Io(error));
        }
        return cancel_and_reap(handle, overlapped, PipeError::Io(error));
    }
}

/// `HasOverlappedIoCompleted`: the kernel replaces the pending status once the
/// operation has finished, whatever its outcome.
fn overlapped_has_completed(overlapped: &OVERLAPPED) -> bool {
    overlapped.Internal != STATUS_PENDING as usize
}

fn cancel_and_reap(
    handle: HANDLE,
    overlapped: &OVERLAPPED,
    original: PipeError,
) -> Result<u32, PipeError> {
    CANCELLED_OPERATIONS.fetch_add(1, Ordering::Relaxed);
    // SAFETY: `handle` is live and `overlapped` names the pending operation
    // that this function always reaps before either value can cease to be
    // valid.
    let cancellation = unsafe { CancelIoEx(handle, overlapped) };
    let cancellation_error = if cancellation == 0 {
        let error = io::Error::last_os_error();
        (win32_code(&error) != Some(ERROR_NOT_FOUND)).then_some(error)
    } else {
        None
    };
    let mut transferred = 0;
    // SAFETY: the same live handle and OVERLAPPED are retained after
    // cancellation; waiting here completes the mandatory reap before their
    // backing storage drops.
    unsafe { GetOverlappedResult(handle, overlapped, &mut transferred, TRUE) };
    match cancellation_error {
        Some(error) => Err(PipeError::Cancel(error)),
        None => Err(original),
    }
}

fn win32_code(error: &io::Error) -> Option<u32> {
    error.raw_os_error().map(|code| code as u32)
}

#[cfg(test)]
fn cancelled_operations() -> u64 {
    CANCELLED_OPERATIONS.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use super::{
        cancelled_operations, PipeAccess, PipeClient, PipeDirection, PipeError, PipeServer,
        PipeWait,
    };
    use crate::process::Process;
    use crate::security::SecurityDescriptor;

    static NEXT_NONCE: AtomicU64 = AtomicU64::new(0);
    static SERIAL: Mutex<()> = Mutex::new(());

    fn pipe_name() -> String {
        format!(
            r"\\.\pipe\win-custody-tests-{}-{}",
            std::process::id(),
            NEXT_NONCE.fetch_add(1, Ordering::Relaxed)
        )
    }

    fn descriptor() -> SecurityDescriptor {
        SecurityDescriptor::from_sddl("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;OW)").unwrap()
    }

    fn pattern(length: usize, modulus: usize) -> Vec<u8> {
        (0..length).map(|index| (index % modulus) as u8).collect()
    }

    #[test]
    fn non_local_names_are_rejected() {
        for name in [r"\\server\pipe\x", r"\\.\pipe\", "plain"] {
            let error =
                PipeServer::create(name, PipeDirection::Inbound, &descriptor()).unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        }
    }

    #[test]
    fn a_second_server_with_the_same_name_is_refused() {
        let name = pipe_name();
        let _first = PipeServer::create(&name, PipeDirection::Inbound, &descriptor()).unwrap();
        assert!(PipeServer::create(&name, PipeDirection::Inbound, &descriptor()).is_err());
    }

    #[test]
    fn an_outbound_server_transfers_a_large_stream_and_sees_the_client_pid() {
        let _serial = SERIAL.lock().unwrap();
        let name = pipe_name();
        let server = PipeServer::create(&name, PipeDirection::Outbound, &descriptor()).unwrap();
        let expected = pattern(200 * 1024, 251);
        let client_name = name.clone();
        let client = std::thread::spawn(move || {
            let client = PipeClient::open(
                &client_name,
                PipeAccess::Read,
                Instant::now() + Duration::from_secs(5),
                Duration::from_millis(5),
            )
            .unwrap();
            assert_eq!(client.server_process_id().unwrap(), std::process::id());
            let mut received = Vec::new();
            let mut chunk = vec![0_u8; 64 * 1024];
            while received.len() < 200 * 1024 {
                let read = client
                    .read(&mut chunk, &PipeWait::within(Duration::from_secs(5)))
                    .unwrap();
                if read == 0 {
                    break;
                }
                received.extend_from_slice(&chunk[..read]);
            }
            received
        });
        server
            .connect(&PipeWait::within(Duration::from_secs(5)))
            .unwrap();
        assert_eq!(server.client_process_id().unwrap(), std::process::id());
        server
            .write_all(&expected, &PipeWait::within(Duration::from_secs(5)))
            .unwrap();
        assert_eq!(client.join().unwrap(), expected);
    }

    #[test]
    fn a_duplex_pipe_carries_a_request_and_a_reply() {
        let _serial = SERIAL.lock().unwrap();
        let name = pipe_name();
        let server = PipeServer::create(&name, PipeDirection::Duplex, &descriptor()).unwrap();
        let client_name = name.clone();
        let client = std::thread::spawn(move || {
            let client = PipeClient::open(
                &client_name,
                PipeAccess::ReadWrite,
                Instant::now() + Duration::from_secs(5),
                Duration::from_millis(5),
            )
            .unwrap();
            let wait = PipeWait::within(Duration::from_secs(5));
            client.write_all(b"ping", &wait).unwrap();
            let mut reply = [0_u8; 4];
            let read = client.read(&mut reply, &wait).unwrap();
            reply[..read].to_vec()
        });
        let wait = PipeWait::within(Duration::from_secs(5));
        server.connect(&wait).unwrap();
        let mut request = [0_u8; 4];
        let read = server.read(&mut request, &wait).unwrap();
        assert_eq!(&request[..read], b"ping");
        server.write_all(b"pong", &wait).unwrap();
        assert_eq!(client.join().unwrap(), b"pong");
    }

    #[test]
    fn a_connect_past_its_deadline_is_cancelled_and_reaped_once() {
        let _serial = SERIAL.lock().unwrap();
        let server =
            PipeServer::create(&pipe_name(), PipeDirection::Outbound, &descriptor()).unwrap();
        let before = cancelled_operations();
        let result = server.connect(&PipeWait {
            deadline: Instant::now() + Duration::from_millis(25),
            poll: Duration::from_millis(5),
            peer: None,
        });
        assert!(matches!(result, Err(PipeError::TimedOut)));
        assert_eq!(cancelled_operations(), before + 1);
    }

    #[test]
    fn an_exited_peer_ends_a_connect_early() {
        let _serial = SERIAL.lock().unwrap();
        let mut child = std::process::Command::new("cmd")
            .args(["/d", "/c", "exit 0"])
            .spawn()
            .unwrap();
        child.wait().unwrap();
        let peer = Process::from_child(&child).unwrap();
        let server =
            PipeServer::create(&pipe_name(), PipeDirection::Outbound, &descriptor()).unwrap();
        let before = cancelled_operations();
        let result = server.connect(&PipeWait::within(Duration::from_secs(5)).with_peer(&peer));
        assert!(matches!(result, Err(PipeError::PeerExited)));
        assert_eq!(cancelled_operations(), before + 1);
    }
}
