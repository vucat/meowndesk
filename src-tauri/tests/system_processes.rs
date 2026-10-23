//! Linux regression: sysinfo task rows must not duplicate a process in either ranking input.
#![cfg(target_os = "linux")]
use meowndesk_lib::telemetry::{
    clock::FakeClock,
    providers::{PollContext, Provider, system::SystemProvider},
};
use std::{
    process::{Child, Command},
    sync::mpsc,
    thread::JoinHandle,
    time::Duration,
};
struct Workload {
    child: Child,
    stops: Vec<mpsc::Sender<()>>,
    threads: Vec<JoinHandle<()>>,
}
impl Drop for Workload {
    fn drop(&mut self) {
        for stop in self.stops.drain(..) {
            let _ = stop.send(());
        }
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[test]
fn excludes_user_threads_without_excluding_real_child_processes() {
    let mut workload = Workload {
        child: Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("start test-owned child"),
        stops: vec![],
        threads: vec![],
    };
    let mut tids = Vec::new();
    for name in [
        "Flycheck0",
        "LspServer",
        "LspServerWriter",
        "RayonWorker0",
        "RayonWorker1",
    ] {
        let (ready, tid) = mpsc::channel();
        let (stop, done) = mpsc::channel();
        workload.stops.push(stop);
        workload.threads.push(
            std::thread::Builder::new()
                .name(name.into())
                .spawn(move || {
                    // SAFETY: gettid has no pointer arguments or preconditions; this is our own thread.
                    let id = unsafe { libc::syscall(libc::SYS_gettid) } as u32;
                    ready.send(id).expect("report worker TID");
                    let _ = done.recv();
                })
                .expect("start named worker"),
        );
        tids.push(
            tid.recv_timeout(Duration::from_secs(5))
                .expect("worker ready"),
        );
    }
    let provider = SystemProvider::new();
    let clock = FakeClock::new(1000, 1000);
    let poll = provider.poll(&PollContext {
        clock: &clock,
        elapsed_ms: 0,
        interval_ms: 1000,
    });
    assert!(
        poll.processes.iter().any(|p| p.pid == std::process::id()),
        "retain process leader"
    );
    assert!(
        poll.processes.iter().any(|p| p.pid == workload.child.id()),
        "retain genuine child process"
    );
    for tid in tids {
        assert!(
            !poll.processes.iter().any(|p| p.pid == tid),
            "worker TID {tid} duplicates the leader's resident memory"
        );
    }
    for process in poll.processes {
        // Independently verify the emitted table is process leaders, not Linux task IDs.
        if let Ok(status) = std::fs::read_to_string(format!("/proc/{}/status", process.pid)) {
            let tgid = status
                .lines()
                .find_map(|line| line.strip_prefix("Tgid:"))
                .and_then(|id| id.trim().parse::<u32>().ok());
            assert_eq!(tgid, Some(process.pid));
        }
    }
}
