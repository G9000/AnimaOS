//! Creating child processes that inherit exactly the handles they are given.
#![allow(unsafe_code)]

use std::cmp::Ordering;
use std::ffi::{OsStr, OsString};
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, OwnedHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    DuplicateHandle, SetHandleInformation, DUPLICATE_SAME_ACCESS, ERROR_BROKEN_PIPE, HANDLE,
    HANDLE_FLAG_INHERIT,
};
use windows_sys::Win32::Globalization::{
    CompareStringOrdinal, CSTR_EQUAL, CSTR_GREATER_THAN, CSTR_LESS_THAN,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessAsUserW, CreateProcessW, DeleteProcThreadAttributeList,
    InitializeProcThreadAttributeList, ResumeThread, TerminateProcess, UpdateProcThreadAttribute,
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW,
};

use crate::handle::{current_process, owned_from_creation};
use crate::job::Job;
use crate::process::{Process, ProcessAccess};
use crate::session::current_session_id;
use crate::token::PrimaryToken;
use crate::wide::wide_null;

/// Flags the launch manages itself; a caller cannot turn them on or off
/// through [`Launch::creation_flags`].
const MANAGED_FLAGS: u32 =
    CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT;

/// A child process to create, and everything it is allowed to receive.
///
/// Unlike [`std::process::Command`], a launch never lets the child inherit a
/// handle it was not explicitly given: the standard handles and the handles
/// passed to [`Launch::inherit`] form a `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`,
/// and every other inheritable handle in this process stays private.
///
/// The executable is not searched on `PATH`; pass a full path.
///
/// # Standard handles
///
/// If none of [`stdin`](Self::stdin), [`stdout`](Self::stdout) and
/// [`stderr`](Self::stderr) is set, the child starts without standard handles.
/// If any is set, the unset ones are connected to the `NUL` device.
///
/// # Environment
///
/// The child inherits this process's environment unless
/// [`env`](Self::env), [`env_remove`](Self::env_remove) or
/// [`env_clear`](Self::env_clear) is used, in which case it receives a
/// sorted Unicode block built from those changes. A child started with
/// [`as_user`](Self::as_user) still inherits *this* process's environment by
/// default, not the target user's.
#[derive(Debug)]
pub struct Launch<'a> {
    executable: PathBuf,
    arguments: Vec<OsString>,
    /// Private duplicates of the handles the child inherits. They are made
    /// inheritable only for the duration of the creating call.
    inherited: Vec<OwnedHandle>,
    stdin: Option<BorrowedHandle<'a>>,
    stdout: Option<BorrowedHandle<'a>>,
    stderr: Option<BorrowedHandle<'a>>,
    current_dir: Option<PathBuf>,
    environment: EnvironmentPlan,
    create_no_window: bool,
    extra_flags: u32,
    token: Option<&'a PrimaryToken>,
    desktop: Option<OsString>,
}

impl<'a> Launch<'a> {
    /// A launch of `executable` with no arguments, no inherited handles, no
    /// console window, and this process's environment.
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            arguments: Vec::new(),
            inherited: Vec::new(),
            stdin: None,
            stdout: None,
            stderr: None,
            current_dir: None,
            environment: EnvironmentPlan::default(),
            create_no_window: true,
            extra_flags: 0,
            token: None,
            desktop: None,
        }
    }

    /// Appends one argument. It reaches the child exactly as given, quoted
    /// the way `CommandLineToArgvW` parses command lines.
    pub fn arg(&mut self, argument: impl AsRef<OsStr>) -> &mut Self {
        self.arguments.push(argument.as_ref().to_owned());
        self
    }

    /// Appends several arguments.
    pub fn args<I, S>(&mut self, arguments: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.arguments.extend(
            arguments
                .into_iter()
                .map(|argument| argument.as_ref().to_owned()),
        );
        self
    }

    /// Lets the child inherit a copy of `handle` and returns the handle value
    /// the child will see, for passing on its command line or in its
    /// environment.
    ///
    /// The copy is private to this launch and only becomes inheritable while
    /// the child is being created. Other code in this process that creates
    /// processes with blanket inheritance during that call (for example
    /// [`std::process::Command`]) can still inherit it, as with any
    /// inheritable handle; spawn from one place if that matters.
    pub fn inherit(&mut self, handle: &impl AsHandle) -> io::Result<usize> {
        let copy = duplicate(handle.as_handle(), false)?;
        let value = copy.as_raw_handle() as usize;
        self.inherited.push(copy);
        Ok(value)
    }

    /// The child's standard input.
    pub fn stdin<H: AsHandle + ?Sized>(&mut self, handle: &'a H) -> &mut Self {
        self.stdin = Some(handle.as_handle());
        self
    }

    /// The child's standard output.
    pub fn stdout<H: AsHandle + ?Sized>(&mut self, handle: &'a H) -> &mut Self {
        self.stdout = Some(handle.as_handle());
        self
    }

    /// The child's standard error.
    pub fn stderr<H: AsHandle + ?Sized>(&mut self, handle: &'a H) -> &mut Self {
        self.stderr = Some(handle.as_handle());
        self
    }

    /// The child's working directory. Defaults to this process's.
    pub fn current_dir(&mut self, directory: impl Into<PathBuf>) -> &mut Self {
        self.current_dir = Some(directory.into());
        self
    }

    /// Sets an environment variable for the child. Names are compared without
    /// regard to case, as Windows does.
    pub fn env(&mut self, name: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> &mut Self {
        self.environment
            .changes
            .push((name.as_ref().to_owned(), Some(value.as_ref().to_owned())));
        self
    }

    /// Removes an environment variable from the child's environment.
    pub fn env_remove(&mut self, name: impl AsRef<OsStr>) -> &mut Self {
        self.environment
            .changes
            .push((name.as_ref().to_owned(), None));
        self
    }

    /// Starts the child's environment empty instead of from this process's.
    /// Variables set with [`env`](Self::env) are still applied.
    pub fn env_clear(&mut self) -> &mut Self {
        self.environment.clear = true;
        self.environment.changes.clear();
        self
    }

    /// Whether a console child gets no console window (`CREATE_NO_WINDOW`).
    /// Defaults to `true`.
    pub fn create_no_window(&mut self, enabled: bool) -> &mut Self {
        self.create_no_window = enabled;
        self
    }

    /// Additional `CreateProcess` creation flags, such as a priority class or
    /// `CREATE_NEW_PROCESS_GROUP`. `CREATE_SUSPENDED`,
    /// `EXTENDED_STARTUPINFO_PRESENT` and `CREATE_UNICODE_ENVIRONMENT` are
    /// managed by the launch and ignored here.
    pub fn creation_flags(&mut self, flags: u32) -> &mut Self {
        self.extra_flags = flags & !MANAGED_FLAGS;
        self
    }

    /// Creates the child with `token` through `CreateProcessAsUserW`, so it
    /// runs as that token's user in that token's session.
    ///
    /// The caller needs `SeIncreaseQuotaPrivilege`, and
    /// `SeAssignPrimaryTokenPrivilege` unless the token is derived from its
    /// own; services running as LocalSystem hold both. Windows does not let a
    /// child in another session inherit handles, so a launch into another
    /// session must not set standard handles or call
    /// [`inherit`](Self::inherit).
    pub fn as_user(&mut self, token: &'a PrimaryToken) -> &mut Self {
        self.token = Some(token);
        self
    }

    /// The window station and desktop the child starts on, such as
    /// `winsta0\default`.
    pub fn desktop(&mut self, name: impl AsRef<OsStr>) -> &mut Self {
        self.desktop = Some(name.as_ref().to_owned());
        self
    }

    /// Creates the child suspended. Nothing in it runs until
    /// [`SuspendedChild::resume`].
    pub fn spawn_suspended(&mut self) -> io::Result<SuspendedChild> {
        let application = wide_null(&self.executable)?;
        let mut command_line = command_line(&self.executable, &self.arguments)?;
        let current_dir = self.current_dir.as_ref().map(wide_null).transpose()?;
        let environment = self.environment.block()?;
        let mut desktop = self.desktop.as_ref().map(wide_null).transpose()?;

        let standard = if self.stdin.is_some() || self.stdout.is_some() || self.stderr.is_some() {
            Some([
                standard_slot(self.stdin)?,
                standard_slot(self.stdout)?,
                standard_slot(self.stderr)?,
            ])
        } else {
            None
        };
        let mut handles: Vec<HANDLE> = Vec::new();
        if let Some(standard) = &standard {
            handles.extend(standard.iter().map(AsRawHandle::as_raw_handle));
        }
        handles.extend(self.inherited.iter().map(AsRawHandle::as_raw_handle));

        if let Some(token) = self.token {
            if !handles.is_empty() && token.session_id()? != current_session_id()? {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "a child in another session cannot inherit handles",
                ));
            }
        }

        let mut attributes = if handles.is_empty() {
            None
        } else {
            Some(AttributeList::with_handles(&handles)?)
        };
        let inheritable = InheritableWindow::open(&self.inherited)?;

        let mut startup = STARTUPINFOEXW::default();
        let mut flags = self.extra_flags | CREATE_SUSPENDED;
        if let Some(list) = attributes.as_mut() {
            startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
            startup.lpAttributeList = list.as_mut_ptr();
            flags |= EXTENDED_STARTUPINFO_PRESENT;
        } else {
            startup.StartupInfo.cb = size_of::<STARTUPINFOW>() as u32;
        }
        if let Some([input, output, error]) = &standard {
            startup.StartupInfo.dwFlags |= STARTF_USESTDHANDLES;
            startup.StartupInfo.hStdInput = input.as_raw_handle();
            startup.StartupInfo.hStdOutput = output.as_raw_handle();
            startup.StartupInfo.hStdError = error.as_raw_handle();
        }
        if let Some(desktop) = desktop.as_mut() {
            startup.StartupInfo.lpDesktop = desktop.as_mut_ptr();
        }
        if self.create_no_window {
            flags |= CREATE_NO_WINDOW;
        }
        if environment.is_some() {
            flags |= CREATE_UNICODE_ENVIRONMENT;
        }
        let environment_pointer = environment
            .as_ref()
            .map_or(null(), |block| block.as_ptr().cast());
        let directory_pointer = current_dir
            .as_ref()
            .map_or(null(), |directory| directory.as_ptr());
        let inherit_handles = i32::from(!handles.is_empty());

        let mut information = PROCESS_INFORMATION::default();
        let created = match self.token {
            // SAFETY: `application`, `directory_pointer` (when non-null) and
            // the writable `command_line` are NUL-terminated buffers that
            // outlive the call; the environment pointer, when non-null, is a
            // double-NUL-terminated UTF-16 block flagged with
            // CREATE_UNICODE_ENVIRONMENT; `startup` carries live standard
            // handles and, when present, an initialized attribute list whose
            // handles are all inheritable for this call; `information` is a
            // local out structure.
            None => unsafe {
                CreateProcessW(
                    application.as_ptr(),
                    command_line.as_mut_ptr(),
                    null(),
                    null(),
                    inherit_handles,
                    flags,
                    environment_pointer,
                    directory_pointer,
                    &startup.StartupInfo,
                    &mut information,
                )
            },
            // SAFETY: as above; the token is a live primary token borrowed for
            // the lifetime of this launch.
            Some(token) => unsafe {
                CreateProcessAsUserW(
                    token.as_raw_handle(),
                    application.as_ptr(),
                    command_line.as_mut_ptr(),
                    null(),
                    null(),
                    inherit_handles,
                    flags,
                    environment_pointer,
                    directory_pointer,
                    &startup.StartupInfo,
                    &mut information,
                )
            },
        };
        // Read the error before cleanup makes further Win32 calls.
        let failure = (created == 0).then(io::Error::last_os_error);
        drop(inheritable);
        drop(attributes);
        drop(standard);
        if let Some(error) = failure {
            return Err(error);
        }

        // Both handles are owned from here, so neither can leak if the other
        // turns out to be unusable.
        let process = owned_from_creation(information.hProcess);
        let thread = owned_from_creation(information.hThread);
        match (process, thread) {
            (Ok(process), Ok(thread)) => Ok(SuspendedChild {
                process: Some(Process::from_owned(process, ProcessAccess::AsCreated)),
                thread: Some(thread),
                pid: information.dwProcessId,
            }),
            (Ok(process), Err(error)) => {
                // SAFETY: the process handle is live and carries the
                // termination right CreateProcess granted; no pointers.
                unsafe { TerminateProcess(process.as_raw_handle(), 1) };
                Err(error)
            }
            (Err(error), _) => Err(error),
        }
    }

    /// Creates the child suspended, assigns it to `job`, and only then lets it
    /// run, so it is in the job before its first instruction. If any step
    /// fails the child is terminated.
    pub fn spawn_in_job(&mut self, job: &Job) -> io::Result<Process> {
        let child = self.spawn_suspended()?;
        job.assign(child.process())?;
        child.resume()
    }

    /// Creates the child and lets it run.
    pub fn spawn(&mut self) -> io::Result<Process> {
        self.spawn_suspended()?.resume()
    }
}

/// A child created suspended. Dropping it without calling
/// [`resume`](Self::resume) terminates it, so a launch that fails half-way
/// never leaves a frozen process behind.
#[derive(Debug)]
pub struct SuspendedChild {
    process: Option<Process>,
    thread: Option<OwnedHandle>,
    pid: u32,
}

impl SuspendedChild {
    /// The child's process ID.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// The child's process handle, for job assignment and identity checks
    /// before it runs.
    pub fn process(&self) -> &Process {
        self.process.as_ref().expect("present until consumed")
    }

    /// Starts the child's initial thread and hands over the process handle.
    /// If the thread cannot be resumed, the child is terminated.
    pub fn resume(mut self) -> io::Result<Process> {
        let thread = self.thread.take().expect("present until consumed");
        // SAFETY: the thread handle is live and owned; the call takes no
        // pointers.
        if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
            return Err(io::Error::last_os_error());
        }
        Ok(self.process.take().expect("present until consumed"))
    }

    /// Terminates the child without ever letting it run.
    pub fn terminate(mut self, exit_code: u32) -> io::Result<()> {
        self.process
            .take()
            .expect("present until consumed")
            .terminate(exit_code)
    }
}

impl Drop for SuspendedChild {
    fn drop(&mut self) {
        if let Some(process) = self.process.take() {
            let _ = process.terminate(1);
        }
    }
}

/// An anonymous pipe as `(read, write)`. Neither end is inheritable; give an
/// end to a child with [`Launch::stdout`], [`Launch::stdin`] or
/// [`Launch::inherit`].
pub fn anonymous_pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let mut read = null_mut();
    let mut write = null_mut();
    // SAFETY: both out pointers are locals; null security attributes create
    // non-inheritable ends.
    if unsafe { CreatePipe(&mut read, &mut write, null(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let read = owned_from_creation(read);
    let write = owned_from_creation(write);
    Ok((read?, write?))
}

/// A read/write handle to the `NUL` device, which discards writes and reads
/// as end-of-file.
pub fn null_device() -> io::Result<OwnedHandle> {
    open_null(false)
}

/// Reads `handle` to end-of-stream, keeping at most `maximum + 1` bytes so a
/// caller can tell "exactly the bound" from "over the bound". A broken pipe
/// counts as end-of-stream.
pub fn read_to_end_bounded(handle: &impl AsHandle, maximum: usize) -> io::Result<Vec<u8>> {
    let raw = handle.as_handle().as_raw_handle();
    let mut output = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let mut bytes_read = 0;
        // SAFETY: the handle is live for the call; `buffer` is writable for
        // its full length, which is what the length argument states.
        let read = unsafe {
            ReadFile(
                raw,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                &mut bytes_read,
                null_mut(),
            )
        };
        if read == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) {
                break;
            }
            return Err(error);
        }
        if bytes_read == 0 {
            break;
        }
        let remaining = maximum.saturating_add(1).saturating_sub(output.len());
        output.extend_from_slice(&buffer[..(bytes_read as usize).min(remaining)]);
    }
    Ok(output)
}

/// An inheritable copy of a standard handle for one launch, or the `NUL`
/// device when the slot was left unset.
fn standard_slot(handle: Option<BorrowedHandle<'_>>) -> io::Result<OwnedHandle> {
    match handle {
        Some(handle) => duplicate(handle, true),
        None => open_null(true),
    }
}

fn duplicate(handle: BorrowedHandle<'_>, inheritable: bool) -> io::Result<OwnedHandle> {
    let mut copy = null_mut();
    // SAFETY: the source handle is live for the call, both process arguments
    // are the current-process pseudo-handle, and `copy` is a local out value.
    let duplicated = unsafe {
        DuplicateHandle(
            current_process(),
            handle.as_raw_handle(),
            current_process(),
            &mut copy,
            0,
            i32::from(inheritable),
            DUPLICATE_SAME_ACCESS,
        )
    };
    if duplicated == 0 {
        return Err(io::Error::last_os_error());
    }
    owned_from_creation(copy)
}

fn open_null(inheritable: bool) -> io::Result<OwnedHandle> {
    let security = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: i32::from(inheritable),
    };
    let name = wide_null("NUL")?;
    // SAFETY: `name` is NUL-terminated and the attribute block outlives the
    // call; no template handle is passed.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            FILE_GENERIC_READ | FILE_GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &security,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    owned_from_creation(handle)
}

/// Makes the launch's private handle copies inheritable, and makes them
/// private again when dropped.
struct InheritableWindow<'h> {
    handles: &'h [OwnedHandle],
    opened: usize,
}

impl<'h> InheritableWindow<'h> {
    fn open(handles: &'h [OwnedHandle]) -> io::Result<Self> {
        let mut window = Self { handles, opened: 0 };
        for handle in handles {
            set_inheritable(handle.as_raw_handle(), true)?;
            window.opened += 1;
        }
        Ok(window)
    }
}

impl Drop for InheritableWindow<'_> {
    fn drop(&mut self) {
        for handle in &self.handles[..self.opened] {
            let _ = set_inheritable(handle.as_raw_handle(), false);
        }
    }
}

fn set_inheritable(handle: RawHandle, inheritable: bool) -> io::Result<()> {
    let value = if inheritable { HANDLE_FLAG_INHERIT } else { 0 };
    // SAFETY: the handle is owned by the launch and live; the mask and value
    // are integers.
    if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, value) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// A `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` attribute list. The storage is
/// allocated to the size the OS reports and deleted when dropped.
struct AttributeList {
    storage: Vec<usize>,
    initialized: bool,
    /// The list stores a pointer to the handle array, so the array lives here.
    _handles: Vec<HANDLE>,
}

impl AttributeList {
    fn with_handles(handles: &[HANDLE]) -> io::Result<Self> {
        let mut bytes = 0_usize;
        // SAFETY: a null list with a size probe is the documented way to ask
        // for the required byte count; the out pointer is a local.
        unsafe { InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut bytes) };
        if bytes == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut list = Self {
            storage: vec![0_usize; bytes.div_ceil(size_of::<usize>())],
            initialized: false,
            _handles: handles.to_vec(),
        };
        // SAFETY: `storage` is at least `bytes` long and word-aligned, and the
        // list is deleted in `Drop` only after this initialization succeeds.
        let initialized =
            unsafe { InitializeProcThreadAttributeList(list.as_mut_ptr(), 1, 0, &mut bytes) };
        if initialized == 0 {
            return Err(io::Error::last_os_error());
        }
        list.initialized = true;
        let pointer = list._handles.as_ptr();
        let length = std::mem::size_of_val(list._handles.as_slice());
        // SAFETY: the list is initialized; the handle array is owned by the
        // list, does not move while the list lives, and its byte length is
        // passed alongside its pointer.
        let updated = unsafe {
            UpdateProcThreadAttribute(
                list.as_mut_ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                pointer.cast(),
                length,
                null_mut(),
                null(),
            )
        };
        if updated == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(list)
    }

    fn as_mut_ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        if self.initialized {
            // SAFETY: the list was initialized in `with_handles` and has not
            // been deleted since.
            unsafe { DeleteProcThreadAttributeList(self.as_mut_ptr()) };
        }
    }
}

/// Environment changes requested for a launch.
#[derive(Debug, Default)]
struct EnvironmentPlan {
    clear: bool,
    /// In call order; `None` removes the variable.
    changes: Vec<(OsString, Option<OsString>)>,
}

impl EnvironmentPlan {
    /// The Unicode environment block, or `None` to inherit this process's.
    fn block(&self) -> io::Result<Option<Vec<u16>>> {
        if !self.clear && self.changes.is_empty() {
            return Ok(None);
        }
        let mut entries: Vec<(Vec<u16>, Vec<u16>)> = Vec::new();
        if !self.clear {
            for (name, value) in std::env::vars_os() {
                upsert(
                    &mut entries,
                    name.encode_wide().collect(),
                    value.encode_wide().collect(),
                );
            }
        }
        for (name, value) in &self.changes {
            let name: Vec<u16> = name.encode_wide().collect();
            if name.is_empty() || name.contains(&0) || name.contains(&u16::from(b'=')) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "environment names must be non-empty and contain neither '=' nor NUL",
                ));
            }
            match value {
                Some(value) => {
                    let value: Vec<u16> = value.encode_wide().collect();
                    if value.contains(&0) {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "environment values must not contain NUL",
                        ));
                    }
                    upsert(&mut entries, name, value);
                }
                None => entries.retain(|(existing, _)| {
                    compare_environment_names(existing, &name) != Ordering::Equal
                }),
            }
        }
        // CreateProcess requires the block sorted by name, without regard to
        // case.
        entries.sort_by(|left, right| compare_environment_names(&left.0, &right.0));
        let mut block = Vec::new();
        for (name, value) in entries {
            block.extend(name);
            block.push(u16::from(b'='));
            block.extend(value);
            block.push(0);
        }
        if block.is_empty() {
            // An empty Unicode block is two NUL units.
            block.push(0);
        }
        block.push(0);
        Ok(Some(block))
    }
}

fn upsert(entries: &mut Vec<(Vec<u16>, Vec<u16>)>, name: Vec<u16>, value: Vec<u16>) {
    entries.retain(|(existing, _)| compare_environment_names(existing, &name) != Ordering::Equal);
    entries.push((name, value));
}

fn compare_environment_names(left: &[u16], right: &[u16]) -> Ordering {
    let (Ok(left_length), Ok(right_length)) =
        (i32::try_from(left.len()), i32::try_from(right.len()))
    else {
        return left.cmp(right);
    };
    // SAFETY: both slices are live and their exact lengths are passed, so no
    // terminator is read; the comparison ignores case ordinally.
    let result = unsafe {
        CompareStringOrdinal(left.as_ptr(), left_length, right.as_ptr(), right_length, 1)
    };
    match result {
        CSTR_LESS_THAN => Ordering::Less,
        CSTR_EQUAL => Ordering::Equal,
        CSTR_GREATER_THAN => Ordering::Greater,
        _ => left.cmp(right),
    }
}

/// The full command line: the executable, always quoted, followed by the
/// arguments, quoted only where `CommandLineToArgvW` needs it.
fn command_line(executable: &Path, arguments: &[OsString]) -> io::Result<Vec<u16>> {
    let executable: Vec<u16> = executable.as_os_str().encode_wide().collect();
    if executable.contains(&0) || executable.contains(&u16::from(b'"')) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the executable path contains a NUL or a quote",
        ));
    }
    // The program name is parsed without backslash escapes, so it is wrapped
    // in quotes verbatim.
    let mut line = vec![u16::from(b'"')];
    line.extend(executable);
    line.push(u16::from(b'"'));
    for argument in arguments {
        line.push(u16::from(b' '));
        append_argument(&mut line, argument)?;
    }
    line.push(0);
    Ok(line)
}

/// The arguments alone, NUL-terminated, for `ShellExecuteExW`'s parameters.
pub(crate) fn command_line_tail(arguments: &[OsString]) -> io::Result<Vec<u16>> {
    let mut line = Vec::new();
    for (index, argument) in arguments.iter().enumerate() {
        if index != 0 {
            line.push(u16::from(b' '));
        }
        append_argument(&mut line, argument)?;
    }
    line.push(0);
    Ok(line)
}

fn append_argument(line: &mut Vec<u16>, argument: &OsStr) -> io::Result<()> {
    let units: Vec<u16> = argument.encode_wide().collect();
    if units.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "an argument contains a NUL",
        ));
    }
    let quote = u16::from(b'"');
    let backslash = u16::from(b'\\');
    let needs_quotes = units.is_empty()
        || units
            .iter()
            .any(|unit| matches!(*unit, 0x09..=0x0d | 0x20 | 0x22));
    if !needs_quotes {
        line.extend(units);
        return Ok(());
    }
    line.push(quote);
    let mut backslashes = 0_usize;
    for unit in units {
        if unit == backslash {
            backslashes += 1;
        } else if unit == quote {
            line.extend(std::iter::repeat(backslash).take(backslashes * 2 + 1));
            line.push(unit);
            backslashes = 0;
        } else {
            line.extend(std::iter::repeat(backslash).take(backslashes));
            line.push(unit);
            backslashes = 0;
        }
    }
    line.extend(std::iter::repeat(backslash).take(backslashes * 2));
    line.push(quote);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::Path;
    use std::time::Duration;

    use super::{anonymous_pipe, command_line, command_line_tail, read_to_end_bounded, Launch};
    use crate::handle::WaitOutcome;
    use crate::job::{Job, JobLimits};

    fn cmd() -> std::path::PathBuf {
        let root = std::env::var_os("SystemRoot").expect("SystemRoot is set");
        Path::new(&root).join("System32").join("cmd.exe")
    }

    fn rendered(arguments: &[&str]) -> String {
        let arguments: Vec<OsString> = arguments.iter().map(OsString::from).collect();
        let units = command_line(Path::new(r"C:\a b\x.exe"), &arguments).unwrap();
        String::from_utf16(&units[..units.len() - 1]).unwrap()
    }

    #[test]
    fn arguments_are_quoted_only_where_the_child_parser_needs_it() {
        assert_eq!(rendered(&[]), r#""C:\a b\x.exe""#);
        assert_eq!(rendered(&["plain"]), r#""C:\a b\x.exe" plain"#);
        assert_eq!(rendered(&[""]), r#""C:\a b\x.exe" """#);
        assert_eq!(rendered(&["two words"]), r#""C:\a b\x.exe" "two words""#);
        assert_eq!(
            rendered(&["with \"quote\""]),
            r#""C:\a b\x.exe" "with \"quote\"""#
        );
        assert_eq!(
            rendered(&[r"trailing\ x\"]),
            r#""C:\a b\x.exe" "trailing\ x\\""#
        );
        assert_eq!(rendered(&[r"no\space\"]), r#""C:\a b\x.exe" no\space\"#);
    }

    #[test]
    fn unpaired_surrogates_survive_quoting() {
        let value = OsString::from_wide(&[0xd800, u16::from(b' '), u16::from(b'x')]);
        let units = command_line_tail(&[value]).unwrap();
        assert_eq!(
            units,
            [
                u16::from(b'"'),
                0xd800,
                u16::from(b' '),
                u16::from(b'x'),
                u16::from(b'"'),
                0
            ]
        );
    }

    #[test]
    fn nuls_and_quotes_in_the_program_name_are_rejected() {
        assert!(command_line(Path::new("a\"b.exe"), &[]).is_err());
        assert!(command_line(Path::new("a.exe"), &[OsString::from("a\0b")]).is_err());
    }

    #[test]
    fn a_child_in_a_kill_on_close_job_reports_its_exit_code() {
        let job = Job::create(&JobLimits::new().kill_on_close(true)).unwrap();
        let child = Launch::new(cmd())
            .args(["/d", "/c", "exit 7"])
            .spawn_in_job(&job)
            .unwrap();
        assert_eq!(
            child.wait(Some(Duration::from_secs(30))).unwrap(),
            WaitOutcome::Signaled
        );
        assert_eq!(child.exit_code().unwrap(), Some(7));
    }

    #[test]
    fn closing_the_job_kills_the_child() {
        let job = Job::create(&JobLimits::new().kill_on_close(true)).unwrap();
        let child = Launch::new(cmd())
            .args(["/d", "/c", "ping -n 60 127.0.0.1 >NUL"])
            .spawn_in_job(&job)
            .unwrap();
        assert_eq!(
            child.wait(Some(Duration::from_millis(200))).unwrap(),
            WaitOutcome::TimedOut
        );
        drop(job);
        assert_eq!(
            child.wait(Some(Duration::from_secs(30))).unwrap(),
            WaitOutcome::Signaled
        );
    }

    #[test]
    fn standard_output_and_environment_reach_the_child() {
        let (read, write) = anonymous_pipe().unwrap();
        let child = Launch::new(cmd())
            .args(["/d", "/c", "echo %WIN_CUSTODY_PROBE%"])
            .env("WIN_CUSTODY_PROBE", "custody-ok")
            .stdout(&write)
            .spawn()
            .unwrap();
        drop(write);
        let output = read_to_end_bounded(&read, 4096).unwrap();
        child.wait(Some(Duration::from_secs(30))).unwrap();
        assert_eq!(String::from_utf8_lossy(&output).trim(), "custody-ok");
    }

    #[test]
    fn a_removed_variable_is_absent_in_the_child() {
        std::env::set_var("WIN_CUSTODY_REMOVED", "present");
        let (read, write) = anonymous_pipe().unwrap();
        let child = Launch::new(cmd())
            .args([
                "/d",
                "/c",
                "if defined WIN_CUSTODY_REMOVED (echo yes) else (echo no)",
            ])
            .env_remove("win_custody_removed")
            .stdout(&write)
            .spawn()
            .unwrap();
        drop(write);
        let output = read_to_end_bounded(&read, 4096).unwrap();
        child.wait(Some(Duration::from_secs(30))).unwrap();
        assert_eq!(String::from_utf8_lossy(&output).trim(), "no");
    }

    #[test]
    fn a_dropped_suspended_child_is_terminated() {
        let child = Launch::new(cmd())
            .args(["/d", "/c", "exit 0"])
            .spawn_suspended()
            .unwrap();
        let process = crate::process::Process::open(
            child.pid(),
            crate::process::ProcessAccess::QueryLimitedAndSynchronize,
        )
        .unwrap();
        drop(child);
        assert_eq!(
            process.wait(Some(Duration::from_secs(30))).unwrap(),
            WaitOutcome::Signaled
        );
        assert_eq!(process.exit_code().unwrap(), Some(1));
    }

    #[test]
    fn output_beyond_the_bound_is_cut_at_one_extra_byte() {
        let (read, write) = anonymous_pipe().unwrap();
        let child = Launch::new(cmd())
            .args(["/d", "/c", "echo 0123456789"])
            .stdout(&write)
            .spawn()
            .unwrap();
        drop(write);
        let output = read_to_end_bounded(&read, 4).unwrap();
        child.wait(Some(Duration::from_secs(30))).unwrap();
        assert_eq!(output, b"01234");
    }
}
