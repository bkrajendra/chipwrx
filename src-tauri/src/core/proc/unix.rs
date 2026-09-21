//! Unix process-group control. `tokio::process::Command::process_group(0)` puts the child
//! in a new process group whose pgid equals its own pid, so `SCons`' compiler grandchildren
//! (which inherit the group) die with one signal to `-pgid`.

use super::Killer;
use std::os::unix::process::CommandExt;
use tokio::process::Command;

pub fn prepare(cmd: &mut Command) {
    cmd.process_group(0);
}

pub struct UnixKiller {
    /// Equal to the child's pid — `process_group(0)` guarantees pgid == pid.
    pgid: i32,
}

impl UnixKiller {
    pub fn new(pid: u32) -> Self {
        Self { pgid: pid as i32 }
    }
}

impl Killer for UnixKiller {
    fn interrupt(&self) -> std::io::Result<()> {
        send(self.pgid, libc::SIGINT)
    }

    fn terminate(&self) -> std::io::Result<()> {
        send(self.pgid, libc::SIGTERM)
    }

    fn force_kill(&self) -> std::io::Result<()> {
        send(self.pgid, libc::SIGKILL)
    }
}

fn send(pgid: i32, sig: i32) -> std::io::Result<()> {
    // Negative pid targets the whole process group (`man 2 kill`).
    let rc = unsafe { libc::kill(-pgid, sig) };
    if rc == 0 {
        Ok(())
    } else {
        let err = std::io::Error::last_os_error();
        // ESRCH: the group is already gone — not an error for our purposes.
        if err.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(err)
        }
    }
}
