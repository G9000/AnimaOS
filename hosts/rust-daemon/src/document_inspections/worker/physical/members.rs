//! Exact process witnesses; a PID snapshot alone never proves complete custody.
use std::collections::BTreeMap;
use std::io;
use std::os::windows::io::AsHandle;
use std::time::Duration;
use tokio::time::Instant;
use win_custody::{Job, JobProcessCounts, Process, ProcessAccess, WaitOutcome};

const MAX_WITNESSES: usize = 64;

#[derive(Default)]
pub(super) struct Members {
    processes: BTreeMap<(u32, u64), Process>,
    observed_total: u32,
}

fn validate_total(total: u32, previous: u32) -> io::Result<()> {
    if total > MAX_WITNESSES as u32 || total < previous {
        return Err(io::Error::other(
            "document inspection lifetime process count exceeded its bound or regressed",
        ));
    }
    Ok(())
}

fn validate_counts(
    counts: JobProcessCounts,
    witnessed: usize,
    observed_total: u32,
) -> io::Result<()> {
    validate_total(counts.total, observed_total)?;
    if witnessed > MAX_WITNESSES
        || counts.total > MAX_WITNESSES as u32
        || counts.active != 0
        || counts.total as usize != witnessed
    {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "document inspection lifetime process coverage is incomplete",
        ));
    }
    Ok(())
}

impl Members {
    pub(super) fn capture(&mut self, job: &Job) -> io::Result<()> {
        let total = job.process_counts()?.total;
        validate_total(total, self.observed_total)?;
        self.observed_total = total;
        for pid in job.process_ids()? {
            // Retaining the handle prevents this PID being recycled while its
            // witness is in the ledger. Creation time also binds its identity.
            if self.processes.keys().any(|(known, _)| *known == pid) {
                continue;
            }
            if self.processes.len() >= MAX_WITNESSES {
                return Err(io::Error::other(
                    "document inspection process witness limit exceeded",
                ));
            }
            let process = Process::open(pid, ProcessAccess::QueryLimitedAndSynchronize)?;
            if process.pid()? != pid || !job.contains(&process)? {
                return Err(io::Error::other(
                    "document inspection process membership changed",
                ));
            }
            let identity = (pid, process.creation_time()?);
            if self.processes.insert(identity, process).is_some() {
                return Err(io::Error::other(
                    "document inspection process witness was duplicated",
                ));
            }
        }
        Ok(())
    }

    pub(super) async fn wait_for_signals(&self, deadline: Instant) -> io::Result<()> {
        let handles = self
            .processes
            .values()
            .map(|process| process.as_handle().try_clone_to_owned())
            .collect::<io::Result<Vec<_>>>()?;
        let deadline = deadline.into_std();
        // Wait for the actual kernel process objects with one shared remaining
        // deadline. There is no filesystem cleanup attempt or timing delay here.
        tokio::task::spawn_blocking(move || {
            for handle in handles {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if win_custody::wait(&handle, Some(remaining))? != WaitOutcome::Signaled {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "document inspection process retirement exceeded its deadline",
                    ));
                }
            }
            Ok(())
        })
        .await
        .map_err(|_| io::Error::other("document inspection process wait did not join"))?
    }

    pub(super) fn verify_retired(&self, job: &Job) -> io::Result<()> {
        for process in self.processes.values() {
            if process.wait(Some(Duration::ZERO))? != WaitOutcome::Signaled {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "document inspection process has not retired",
                ));
            }
        }
        // TotalProcesses never resets for this fresh private Job. A process
        // missed between snapshots, a denied creation or a late descendant
        // raises the lifetime count beyond our exact witnesses and fails closed.
        // Once all witnessed processes signal and their count covers the entire
        // lifetime, no unobserved member remains that could create another one.
        validate_counts(
            job.process_counts()?,
            self.processes.len(),
            self.observed_total,
        )
    }

    pub(super) fn clear(&mut self) {
        self.processes.clear();
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.processes.len()
    }

    #[cfg(test)]
    pub(super) fn contains_pid(&self, pid: u32) -> bool {
        self.processes.keys().any(|(known, _)| *known == pid)
    }

    #[cfg(test)]
    pub(super) fn remove_for_test(&mut self, pid: u32) -> Option<((u32, u64), Process)> {
        let identity = self
            .processes
            .keys()
            .find(|(known, _)| *known == pid)
            .copied()?;
        self.processes.remove_entry(&identity)
    }

    #[cfg(test)]
    pub(super) fn restore_for_test(&mut self, witness: ((u32, u64), Process)) {
        assert!(self.processes.insert(witness.0, witness.1).is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missed_lifetime_member_refuses_retirement_even_at_zero_active() {
        assert!(validate_counts(
            JobProcessCounts {
                total: 2,
                active: 0
            },
            1,
            2
        )
        .is_err());
        assert!(validate_counts(
            JobProcessCounts {
                total: 2,
                active: 0
            },
            2,
            2
        )
        .is_ok());
        assert!(validate_counts(
            JobProcessCounts {
                total: 2,
                active: 1
            },
            2,
            2
        )
        .is_err());
    }

    #[test]
    fn excess_or_saturated_lifetime_counts_never_prove_completeness() {
        assert!(validate_counts(
            JobProcessCounts {
                total: u32::MAX,
                active: 0
            },
            1,
            1
        )
        .is_err());
        assert!(validate_counts(
            JobProcessCounts {
                total: 65,
                active: 0
            },
            65,
            1
        )
        .is_err());
        assert!(validate_counts(
            JobProcessCounts {
                total: 1,
                active: 0
            },
            2,
            1
        )
        .is_err());
        assert!(
            validate_counts(
                JobProcessCounts {
                    total: 1,
                    active: 0
                },
                1,
                2
            )
            .is_err(),
            "a regressed or wrapped counter cannot prove complete coverage"
        );
    }
}
