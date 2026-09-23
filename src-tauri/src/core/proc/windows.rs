//! Windows process-tree control via a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
//!
//! `SCons` compiler grandchildren are created by the child without `CREATE_BREAKAWAY_FROM_JOB`,
//! so they join the same job automatically. `TerminateJobObject` kills the whole tree in one
//! call, and closing the last handle to the job (e.g. on process exit, or `WinKiller::drop`)
//! kills any survivors as an OS-level safety net — no explicit "kill everything on app exit"
//! bookkeeping required for the Windows case.

use super::Killer;
use std::ffi::c_void;
use tokio::process::Child;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_BASIC_LIMIT_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, TerminateProcess, CREATE_NEW_PROCESS_GROUP, PROCESS_TERMINATE,
};

/// `M9`: used by `reap_orphans_from_previous_run` against a raw pid from a *previous*
/// process's on-disk registry, not a live `Handle`/`Child` — there's no job-object handle
/// left to terminate-via-close (that died with the previous process), so this opens the pid
/// directly. A pid that's gone at all fails `OpenProcess` and is correctly skipped. A pid
/// the OS has since recycled for an unrelated process is the one real risk this accepts
/// (`SPEC.md` §8 open question 40): if that unrelated process happens to be owned by the
/// same user (most processes are), this *will* terminate it. No generation/start-time check
/// guards against it — narrow window, but not zero. Returns `true` if the pid was alive
/// (and has now been terminated), `false` otherwise.
pub fn kill_if_alive(pid: u32) -> bool {
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if handle.is_null() {
            return false;
        }
        TerminateProcess(handle, 1);
        CloseHandle(handle);
    }
    true
}

pub fn prepare(cmd: &mut tokio::process::Command) {
    // Lets us target the whole tree with GenerateConsoleCtrlEvent for a "soft" interrupt.
    cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
}

/// Creates a job object with kill-on-close semantics and assigns `child` to it. Call right
/// after `spawn()` — there's a small window before this where a pathologically fast child
/// could already have spawned a grandchild outside the job, but in practice (SCons, esptool)
/// that takes far longer than this call.
pub fn assign_to_new_job(child: &Child) -> std::io::Result<isize> {
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return Err(std::io::Error::last_os_error());
        }

        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation = JOBOBJECT_BASIC_LIMIT_INFORMATION {
            LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            ..std::mem::zeroed()
        };
        let ok = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if ok == 0 {
            let err = std::io::Error::last_os_error();
            CloseHandle(job);
            return Err(err);
        }

        let proc_handle = match child.raw_handle() {
            Some(h) => h as HANDLE,
            None => {
                CloseHandle(job);
                return Err(std::io::Error::other("child has no raw handle (already reaped?)"));
            }
        };
        let ok = AssignProcessToJobObject(job, proc_handle);
        if ok == 0 {
            let err = std::io::Error::last_os_error();
            CloseHandle(job);
            return Err(err);
        }

        Ok(job as isize)
    }
}

pub struct WinKiller {
    pid: u32,
    /// Job handle, stored as `isize` so this stays `Send + Sync` without an unsafe impl.
    job: isize,
}

impl WinKiller {
    pub fn new(pid: u32, job: isize) -> Self {
        Self { pid, job }
    }
}

impl Killer for WinKiller {
    fn interrupt(&self) -> std::io::Result<()> {
        let ok = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, self.pid) };
        if ok == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn terminate(&self) -> std::io::Result<()> {
        // No graceful SIGTERM equivalent for an arbitrary console process tree — a job
        // terminate is the "hard" step; ProcessSupervisor::terminate still tries interrupt
        // first at the call-site policy level for processes that do handle CTRL_BREAK.
        self.force_kill()
    }

    fn force_kill(&self) -> std::io::Result<()> {
        let ok = unsafe { TerminateJobObject(self.job as HANDLE, 1) };
        if ok == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

impl Drop for WinKiller {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.job as HANDLE);
        }
    }
}
