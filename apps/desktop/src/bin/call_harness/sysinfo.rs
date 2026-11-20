//! What else was using the machine while a run took place: load average and the busiest processes, read with bounded commands.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Run a command and return its output; give up after `limit`. The output goes to a file, not a
/// pipe, so that a long listing cannot block the command while it is waited for.
fn run_bounded(program: &str, args: &[&str], limit: Duration) -> Option<String> {
    let path = std::env::temp_dir().join(format!(
        "prxvc-sysinfo-{}-{}.txt",
        std::process::id(),
        Instant::now().elapsed().as_nanos() ^ args.len() as u128
    ));
    let file = std::fs::File::create(&path).ok()?;
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::from(file))
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    let finished = loop {
        match child.try_wait() {
            Ok(Some(_)) => break true,
            Ok(None) if started.elapsed() > limit => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => break false,
        }
    };
    let out = std::fs::read_to_string(&path).ok();
    let _ = std::fs::remove_file(&path);
    out.filter(|_| finished)
}

/// One line of load and the busiest processes.
pub(crate) fn snapshot(label: &str) -> String {
    let limit = Duration::from_secs(5);
    let load = run_bounded("/usr/sbin/sysctl", &["-n", "vm.loadavg"], limit)
        .unwrap_or_default()
        .trim()
        .to_owned();
    let processes = run_bounded("/bin/ps", &["-Ao", "pcpu,pmem,comm", "-r"], limit)
        .unwrap_or_default()
        .lines()
        .skip(1)
        .take(6)
        .map(|l| {
            let mut f = l.split_whitespace();
            let (cpu, mem) = (f.next().unwrap_or("?"), f.next().unwrap_or("?"));
            let name = f.collect::<Vec<_>>().join(" ");
            let name = name.rsplit('/').next().unwrap_or(&name).to_owned();
            format!("{name} {cpu}% cpu {mem}% mem")
        })
        .collect::<Vec<_>>()
        .join("; ");
    let count = run_bounded("/bin/ps", &["-Ao", "pid"], limit).map_or(0, |o| o.lines().count());
    format!("{label}: load average {load}; {count} processes; busiest: {processes}")
}
