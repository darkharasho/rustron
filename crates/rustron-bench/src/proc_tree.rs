//! Spawns an engine and measures / kills its whole process tree.
use std::io;
use std::process::{Child, Command};
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
    let child = cmd.spawn()?;
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        let job = win32job::Job::create().map_err(io::Error::other)?;
        let mut info = job.query_extended_limit_info().map_err(io::Error::other)?;
        info.limit_kill_on_job_close();
        job.set_extended_limit_info(&info).map_err(io::Error::other)?;
        job.assign_process(child.as_raw_handle() as isize).map_err(io::Error::other)?;
        return Ok(Spawned { child, job });
    }
    #[cfg(not(windows))]
    Ok(Spawned { child })
}

#[cfg(target_os = "linux")]
fn is_thread(p: &sysinfo::Process) -> bool {
    // sysinfo lists threads as processes on Linux; they share the parent's memory.
    p.thread_kind().is_some()
}
#[cfg(not(target_os = "linux"))]
fn is_thread(_: &sysinfo::Process) -> bool {
    false
}

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

    pub fn tree_memory_mb(&self) -> MemReading {
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let pids = descendants(&sys, self.pid());
        #[cfg(windows)]
        let bytes: u64 = pids
            .iter()
            .filter_map(|p| sys.process(Pid::from_u32(*p)))
            .map(|p| p.memory())
            .sum();
        #[cfg(not(windows))]
        let bytes: u64 = pids.iter().map(|p| pss_bytes(*p)).sum();
        MemReading { mb: bytes as f64 / (1024.0 * 1024.0), kind: MEM_KIND }
    }

    pub fn kill_tree(&mut self) {
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let pids = descendants(&sys, self.pid());
        #[cfg(windows)]
        {
            // Dropping the job kills everything in it; kill the root explicitly too.
            let _ = &self.job;
            for p in &pids {
                if let Some(pr) = sys.process(Pid::from_u32(*p)) {
                    pr.kill();
                }
            }
        }
        #[cfg(not(windows))]
        {
            for p in &pids {
                if let Some(pr) = sys.process(Pid::from_u32(*p)) {
                    pr.kill();
                }
            }
            // Catch stragglers that reparented away from the tree.
            kill_group(self.pid());
        }
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
