//! Job objects that bind a child's lifetime and resources to the parent.
#![allow(unsafe_code)]

use std::io;
use std::mem::size_of;
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, OwnedHandle, RawHandle};
use std::ptr::{null, null_mut};

use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_JOB_MEMORY, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOB_OBJECT_LIMIT_PROCESS_MEMORY,
};

use crate::handle::{current_process, owned_from_creation};
use crate::security::SecurityDescriptor;

/// The limits a [`Job`] enforces.
///
/// Built with the setter methods, which can be chained:
///
/// ```
/// # #[cfg(windows)] {
/// use win_custody::JobLimits;
///
/// let limits = JobLimits::new()
///     .kill_on_close(true)
///     .active_processes(1)
///     .process_memory(256 * 1024 * 1024);
/// assert!(limits.kills_on_close());
/// assert_eq!(limits.active_process_limit(), Some(1));
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct JobLimits {
    kill_on_close: bool,
    active_processes: Option<u32>,
    process_memory: Option<usize>,
    job_memory: Option<usize>,
}

impl JobLimits {
    /// No limits at all.
    pub const fn new() -> Self {
        Self {
            kill_on_close: false,
            active_processes: None,
            process_memory: None,
            job_memory: None,
        }
    }

    /// Kill every process in the job when the last handle to the job closes,
    /// including when the owning process is killed.
    #[must_use]
    pub const fn kill_on_close(mut self, enabled: bool) -> Self {
        self.kill_on_close = enabled;
        self
    }

    /// At most `limit` processes may be active in the job at once. Assigning
    /// or creating one more fails.
    #[must_use]
    pub const fn active_processes(mut self, limit: u32) -> Self {
        self.active_processes = Some(limit);
        self
    }

    /// Each process may commit at most `bytes` of memory.
    #[must_use]
    pub const fn process_memory(mut self, bytes: usize) -> Self {
        self.process_memory = Some(bytes);
        self
    }

    /// All processes in the job together may commit at most `bytes`.
    #[must_use]
    pub const fn job_memory(mut self, bytes: usize) -> Self {
        self.job_memory = Some(bytes);
        self
    }

    /// Whether the job kills its processes when its last handle closes.
    pub const fn kills_on_close(&self) -> bool {
        self.kill_on_close
    }

    /// The active-process limit, if one is set.
    pub const fn active_process_limit(&self) -> Option<u32> {
        self.active_processes
    }

    /// The per-process commit limit in bytes, if one is set.
    pub const fn process_memory_limit(&self) -> Option<usize> {
        self.process_memory
    }

    /// The whole-job commit limit in bytes, if one is set.
    pub const fn job_memory_limit(&self) -> Option<usize> {
        self.job_memory
    }

    fn validate(&self) -> io::Result<()> {
        if self.active_processes == Some(0)
            || self.process_memory == Some(0)
            || self.job_memory == Some(0)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a zero job limit would refuse every process",
            ));
        }
        Ok(())
    }

    fn to_raw(self) -> JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
        let mut raw = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        let basic = &mut raw.BasicLimitInformation;
        if self.kill_on_close {
            basic.LimitFlags |= JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        }
        if let Some(limit) = self.active_processes {
            basic.LimitFlags |= JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
            basic.ActiveProcessLimit = limit;
        }
        if let Some(bytes) = self.process_memory {
            basic.LimitFlags |= JOB_OBJECT_LIMIT_PROCESS_MEMORY;
            raw.ProcessMemoryLimit = bytes;
        }
        if let Some(bytes) = self.job_memory {
            raw.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
            raw.JobMemoryLimit = bytes;
        }
        raw
    }

    fn from_raw(raw: &JOBOBJECT_EXTENDED_LIMIT_INFORMATION) -> Self {
        let flags = raw.BasicLimitInformation.LimitFlags;
        Self {
            kill_on_close: flags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE != 0,
            active_processes: (flags & JOB_OBJECT_LIMIT_ACTIVE_PROCESS != 0)
                .then_some(raw.BasicLimitInformation.ActiveProcessLimit),
            process_memory: (flags & JOB_OBJECT_LIMIT_PROCESS_MEMORY != 0)
                .then_some(raw.ProcessMemoryLimit),
            job_memory: (flags & JOB_OBJECT_LIMIT_JOB_MEMORY != 0).then_some(raw.JobMemoryLimit),
        }
    }
}

/// An anonymous job object whose limits are in force before the first
/// process can join it.
#[derive(Debug)]
pub struct Job(OwnedHandle);

impl Job {
    /// Creates a job with `limits` and the creator's default security.
    pub fn create(limits: &JobLimits) -> io::Result<Self> {
        Self::create_inner(limits, None)
    }

    /// Creates a job with `limits`, protected by `descriptor`.
    pub fn create_with_security(
        limits: &JobLimits,
        descriptor: &SecurityDescriptor,
    ) -> io::Result<Self> {
        Self::create_inner(limits, Some(descriptor))
    }

    fn create_inner(
        limits: &JobLimits,
        descriptor: Option<&SecurityDescriptor>,
    ) -> io::Result<Self> {
        limits.validate()?;
        let attributes = descriptor.map(|descriptor| descriptor.attributes(false));
        let attributes_pointer = attributes
            .as_ref()
            .map_or(null(), |attributes| attributes.as_ptr());
        // SAFETY: the attribute block, when present, and the descriptor it
        // points at outlive the call; no name is passed.
        let handle = unsafe { CreateJobObjectW(attributes_pointer, null()) };
        let job = Self(owned_from_creation(handle)?);
        let raw = limits.to_raw();
        // SAFETY: the job handle is live; the buffer pointer and length
        // describe exactly the local limit structure.
        let set = unsafe {
            SetInformationJobObject(
                job.raw(),
                JobObjectExtendedLimitInformation,
                (&raw as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if set == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    /// Assigns `process` to the job. The handle needs `PROCESS_SET_QUOTA`
    /// and `PROCESS_TERMINATE`, which a process this one created has.
    pub fn assign(&self, process: &impl AsHandle) -> io::Result<()> {
        let process = process.as_handle().as_raw_handle();
        // SAFETY: both handles are live for the call.
        if unsafe { AssignProcessToJobObject(self.raw(), process) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Puts the calling process in the job and keeps the job handle open for
    /// the rest of its life. With [`JobLimits::kill_on_close`], every
    /// descendant then dies with this process, however it ends.
    pub fn assign_current_process(self) -> io::Result<()> {
        // SAFETY: the job handle is live; GetCurrentProcess returns a valid
        // pseudo-handle that does not need to be closed.
        if unsafe { AssignProcessToJobObject(self.raw(), current_process()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // The handle must outlive every process in the job, and this process
        // is one of them, so it is deliberately never closed.
        std::mem::forget(self);
        Ok(())
    }

    /// Terminates every process in the job with `exit_code`.
    pub fn terminate(&self, exit_code: u32) -> io::Result<()> {
        // SAFETY: the handle is live; the call takes no pointers.
        if unsafe { TerminateJobObject(self.raw(), exit_code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Reads the limits back from the kernel.
    pub fn limits(&self) -> io::Result<JobLimits> {
        let mut raw = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        // SAFETY: the handle is live; the buffer pointer and length describe
        // exactly the local limit structure and the returned-length pointer is
        // null, which the API permits.
        let queried = unsafe {
            QueryInformationJobObject(
                self.raw(),
                JobObjectExtendedLimitInformation,
                (&mut raw as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                null_mut(),
            )
        };
        if queried == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(JobLimits::from_raw(&raw))
    }

    fn raw(&self) -> RawHandle {
        self.0.as_raw_handle()
    }
}

impl AsHandle for Job {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.0.as_handle()
    }
}

impl AsRawHandle for Job {
    fn as_raw_handle(&self) -> RawHandle {
        self.raw()
    }
}

impl From<Job> for OwnedHandle {
    fn from(job: Job) -> Self {
        job.0
    }
}

#[cfg(test)]
mod tests {
    use super::{Job, JobLimits};

    #[test]
    fn limits_read_back_as_they_were_set() {
        let limits = JobLimits::new()
            .kill_on_close(true)
            .active_processes(1)
            .process_memory(256 * 1024 * 1024)
            .job_memory(512 * 1024 * 1024);
        let job = Job::create(&limits).unwrap();
        assert_eq!(job.limits().unwrap(), limits);
    }

    #[test]
    fn an_empty_limit_set_reads_back_empty() {
        let job = Job::create(&JobLimits::new()).unwrap();
        assert_eq!(job.limits().unwrap(), JobLimits::new());
    }

    #[test]
    fn zero_limits_are_refused_before_the_job_exists() {
        for limits in [
            JobLimits::new().active_processes(0),
            JobLimits::new().process_memory(0),
            JobLimits::new().job_memory(0),
        ] {
            let error = Job::create(&limits).unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        }
    }
}
