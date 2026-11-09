//! Spawns an engine and measures / kills its whole process tree.
use std::io;
use std::process::{Child, Command};
#[cfg(not(windows))]
use sysinfo::{Pid, ProcessesToUpdate, System};

pub struct MemReading {
    pub mb: f64,
    pub kind: &'static str,
}

pub struct Spawned {
    child: Child,
    #[cfg(windows)]
    job: win32job::Job,
}

#[cfg(windows)]
pub const MEM_KIND: &str = "private bytes";
#[cfg(not(windows))]
pub const MEM_KIND: &str = "PSS";

pub fn spawn(mut cmd: Command) -> io::Result<Spawned> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Own process group so the whole tree can be signalled.
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Start suspended so the process is in the job before it can create children.
        cmd.creation_flags(win::CREATE_SUSPENDED);
    }
    let mut child = cmd.spawn()?;
    #[cfg(windows)]
    {
        return match win::attach_and_resume(&child) {
            Ok(job) => Ok(Spawned { child, job }),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(e)
            }
        };
    }
    #[cfg(not(windows))]
    {
        let _ = &mut child;
        Ok(Spawned { child })
    }
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        JobObjectBasicProcessIdList, QueryInformationJobObject, TerminateJobObject,
        JOBOBJECT_BASIC_PROCESS_ID_LIST,
    };
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    pub const CREATE_SUSPENDED: u32 = 0x0000_0004;

    #[link(name = "ntdll")]
    extern "system" {
        fn NtResumeProcess(process: *mut c_void) -> i32;
    }

    fn err(e: impl std::fmt::Display) -> io::Error {
        io::Error::other(e.to_string())
    }

    pub fn handle(h: isize) -> HANDLE {
        HANDLE(h as *mut c_void)
    }

    /// Puts the (suspended) child into a kill-on-close job, then resumes it.
    /// NtResumeProcess resumes every thread of the process, which for a fresh
    /// CREATE_SUSPENDED process is just the initial thread; it avoids a toolhelp
    /// thread enumeration.
    pub fn attach_and_resume(child: &Child) -> io::Result<win32job::Job> {
        let job = win32job::Job::create().map_err(err)?;
        let mut info = job.query_extended_limit_info().map_err(err)?;
        info.limit_kill_on_job_close();
        job.set_extended_limit_info(&info).map_err(err)?;
        job.assign_process(child.as_raw_handle() as isize).map_err(err)?;
        let status = unsafe { NtResumeProcess(child.as_raw_handle()) };
        if status < 0 {
            return Err(err(format!("NtResumeProcess failed: {status:#x}")));
        }
        Ok(job)
    }

    /// Pids of every process currently in the job, whatever their parent.
    pub fn job_pids(job: &win32job::Job) -> Vec<u32> {
        const MAX: usize = 4096;
        // Header (2 x u32) followed by the pid array, as usize slots.
        let mut buf = vec![0usize; 2 + MAX];
        let ok = unsafe {
            QueryInformationJobObject(
                Some(handle(job.handle())),
                JobObjectBasicProcessIdList,
                buf.as_mut_ptr() as *mut c_void,
                (buf.len() * std::mem::size_of::<usize>()) as u32,
                None,
            )
        };
        if ok.is_err() {
            return Vec::new();
        }
        let list = unsafe { &*(buf.as_ptr() as *const JOBOBJECT_BASIC_PROCESS_ID_LIST) };
        let n = (list.NumberOfProcessIdsInList as usize).min(MAX);
        let ids = unsafe { std::slice::from_raw_parts(list.ProcessIdList.as_ptr(), n) };
        ids.to_vec().into_iter().map(|p| p as u32).collect()
    }

    /// PrivateUsage (private bytes) of one process.
    pub fn private_bytes(pid: u32) -> u64 {
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return 0;
            };
            let mut c = PROCESS_MEMORY_COUNTERS_EX::default();
            let ok = GetProcessMemoryInfo(
                h,
                &mut c as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS,
                std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            );
            let _ = CloseHandle(h);
            if ok.is_ok() { c.PrivateUsage as u64 } else { 0 }
        }
    }

    pub fn terminate(job: &win32job::Job) {
        unsafe {
            let _ = TerminateJobObject(handle(job.handle()), 1);
        }
    }
}

#[cfg(target_os = "linux")]
fn is_thread(p: &sysinfo::Process) -> bool {
    // sysinfo lists threads as processes on Linux; they share the parent's memory.
    p.thread_kind().is_some()
}
#[cfg(all(not(windows), not(target_os = "linux")))]
fn is_thread(_: &sysinfo::Process) -> bool {
    false
}

#[cfg(not(windows))]
fn descendants(sys: &System, root: u32) -> Vec<u32> {
    let mut all = vec![root];
    let mut i = 0;
    while i < all.len() {
        let parent = Pid::from_u32(all[i]);
        for (pid, p) in sys.processes() {
            if p.parent() == Some(parent) && !is_thread(p) && !all.contains(&pid.as_u32()) {
                all.push(pid.as_u32());
            }
        }
        i += 1;
    }
    all
}

impl Spawned {
    /// Takes the child's piped stdout, if the command was spawned with `Stdio::piped()`.
    pub fn take_stdout(&mut self) -> Option<std::process::ChildStdout> {
        self.child.stdout.take()
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    #[cfg(windows)]
    pub fn tree_memory_mb(&self) -> MemReading {
        // Every process actually in the job, regardless of parentage.
        let bytes: u64 = win::job_pids(&self.job).into_iter().map(win::private_bytes).sum();
        MemReading { mb: bytes as f64 / (1024.0 * 1024.0), kind: MEM_KIND }
    }

    #[cfg(not(windows))]
    pub fn tree_memory_mb(&self) -> MemReading {
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let bytes: u64 = descendants(&sys, self.pid()).iter().map(|p| pss_bytes(*p)).sum();
        MemReading { mb: bytes as f64 / (1024.0 * 1024.0), kind: MEM_KIND }
    }

    #[cfg(windows)]
    pub fn kill_tree(&mut self) {
        // Terminates every member of the job synchronously-enough; kill-on-close is the backstop.
        win::terminate(&self.job);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    #[cfg(not(windows))]
    pub fn kill_tree(&mut self) {
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let pids = descendants(&sys, self.pid());
        for p in &pids {
            if let Some(pr) = sys.process(Pid::from_u32(*p)) {
                pr.kill();
            }
        }
        // Catch stragglers that reparented away from the tree.
        kill_group(self.pid());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(not(windows))]
fn kill_group(pgid: u32) {
    // Process group id equals the child's pid (process_group(0)); shell out to avoid libc dep.
    let _ = std::process::Command::new("kill")
        .args(["-KILL", &format!("-{pgid}")])
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(not(windows))]
fn pss_bytes(pid: u32) -> u64 {
    let Ok(s) = std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup")) else {
        return 0;
    };
    s.lines()
        .find_map(|l| l.strip_prefix("Pss:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
        .map_or(0, |kb| kb * 1024)
}
