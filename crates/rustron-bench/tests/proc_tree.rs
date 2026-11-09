use rustron_bench::proc_tree::spawn;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessesToUpdate, System};

fn tree_cmd() -> Command {
    #[cfg(windows)]
    let mut c = {
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-Command", "$p = Start-Process ping -ArgumentList '-n','600','127.0.0.1' -PassThru -WindowStyle Hidden; $p.Id; Wait-Process -Id $p.Id"]);
        c
    };
    #[cfg(not(windows))]
    let mut c = {
        let mut c = Command::new("sh");
        c.args(["-c", "sleep 600 & echo $!; wait"]);
        c
    };
    c.stdout(Stdio::piped());
    c
}

fn alive(pid: u32) -> bool {
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    // zombies count as dead
    sys.process(Pid::from_u32(pid)).is_some_and(|p| p.status() != sysinfo::ProcessStatus::Zombie)
}

fn start() -> (rustron_bench::proc_tree::Spawned, u32) {
    let mut sp = spawn(tree_cmd()).unwrap();
    let out = sp.take_stdout().expect("piped stdout");
    let mut line = String::new();
    BufReader::new(out).read_line(&mut line).unwrap();
    let gc: u32 = line.trim().parse().unwrap();
    (sp, gc)
}

#[test]
fn kills_whole_tree() {
    let (mut sp, gc) = start();
    let parent = sp.pid();
    assert!(alive(gc));
    sp.kill_tree();
    let t = Instant::now();
    while (alive(gc) || alive(parent)) && t.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(!alive(gc), "grandchild alive");
    assert!(!alive(parent), "parent alive");
}

#[test]
fn tree_memory_counts_children() {
    let (mut sp, _gc) = start();
    let m = sp.tree_memory_mb();
    sp.kill_tree();
    assert!(m.mb > 0.0, "{}", m.mb);
    assert_eq!(m.kind, if cfg!(windows) { "private bytes" } else { "PSS" });
}
