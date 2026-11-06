// foamy-kit 0.5.0 cd151f207a538a6447cbb76a86bd3a0029cb0fa964b5f34a967e4784c0b48289 — do not edit; change foamy-kit and run sync
//! This process: dying with the parent, a new session, resource limits and
//! the umask. Child processes get the same through [`crate::child::Bounded`].

use std::io;

pub use rustix::process::Resource;
use rustix::process::{
    Rlimit, Signal, getppid, getrlimit, set_parent_process_death_signal, setrlimit, setsid,
};

/// `PR_SET_PDEATHSIG(SIGKILL)`: the kernel kills this process when its
/// parent (the thread that started it) exits, so a helper never outlives
/// the shell that launched it. Fails if the parent changed while the signal
/// was being set (it exited in between). A parent that exited before this
/// call cannot be detected here (the helper was reparented already); call it
/// first thing in `main`.
pub fn die_with_parent() -> io::Result<()> {
    let parent = getppid();
    set_parent_process_death_signal(Some(Signal::KILL))?;
    if getppid() != parent {
        return Err(io::Error::other("the parent process exited"));
    }
    Ok(())
}

/// `setsid()`: this process leads a new session and process group, detached
/// from the caller's terminal and process-group signals. Fails (`EPERM`)
/// when the process already leads a process group.
pub fn new_session() -> io::Result<()> {
    setsid()?;
    Ok(())
}

/// The soft and hard limit actually set by [`limit`]: `value`, or the
/// current hard limit when that is lower (a limit can only be lowered).
fn bounded(resource: Resource, value: u64) -> Rlimit {
    let value = getrlimit(resource)
        .maximum
        .map_or(value, |hard| hard.min(value));
    Rlimit {
        current: Some(value),
        maximum: Some(value),
    }
}

/// Lowers both the soft and the hard limit of `resource` to `value` (or
/// keeps a lower hard limit), e.g. `limit(Resource::Core, 0)` for no core
/// dumps, `Resource::As` for address space, `Resource::Cpu` for seconds,
/// `Resource::Fsize` for the largest file, `Resource::Nofile` for open files.
/// The hard limit cannot be raised again.
pub fn limit(resource: Resource, value: u64) -> io::Result<()> {
    setrlimit(resource, bounded(resource, value))?;
    Ok(())
}

/// Sets the umask and returns the previous one, e.g. `set_umask(0o077)` so
/// new files and directories are private.
pub fn set_umask(mask: u32) -> u32 {
    rustix::process::umask(rustix::fs::Mode::from_raw_mode(mask & 0o777)).as_raw_mode()
}

/// Child-side setup for [`crate::child::Bounded`]; runs between `fork` and
/// `exec`, so it only makes system calls (no allocation, no locks).
pub(crate) fn child_setup(
    session: bool,
    parent: Option<rustix::process::Pid>,
    limits: &[(Resource, u64)],
) -> io::Result<()> {
    if session {
        setsid()?;
    }
    if let Some(parent) = parent {
        set_parent_process_death_signal(Some(Signal::KILL))?;
        if getppid() != Some(parent) {
            return Err(io::Error::from_raw_os_error(
                rustix::io::Errno::SRCH.raw_os_error(),
            ));
        }
    }
    for &(resource, value) in limits {
        setrlimit(resource, bounded(resource, value))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn umask_round_trips() {
        let previous = set_umask(0o077);
        assert_eq!(set_umask(previous), 0o077);
    }

    #[test]
    fn limits_never_exceed_the_hard_limit() {
        let hard = getrlimit(Resource::Nofile).maximum;
        let wanted = bounded(Resource::Nofile, u64::MAX);
        assert_eq!(wanted.maximum, Some(hard.unwrap_or(u64::MAX)));
        assert_eq!(bounded(Resource::Nofile, 3).current, Some(3));
    }
}
