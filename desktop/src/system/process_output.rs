//! Bounded concurrent draining for small metadata subprocesses.
use std::{
    io,
    process::{Child, Output},
    time::{Duration, Instant},
};
const OUTPUT_LIMIT: usize = 64 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(10);

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(unix)]
pub fn collect(child: Child, timeout: Duration) -> io::Result<Output> {
    use rustix::fs::{OFlags, fcntl_setfl};
    use std::io::Read;
    let mut child = ChildGuard(child);
    let mut stdout = child
        .0
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("missing stdout"))?;
    let mut stderr = child
        .0
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("missing stderr"))?;
    fcntl_setfl(&stdout, OFlags::NONBLOCK)?;
    fcntl_setfl(&stderr, OFlags::NONBLOCK)?;
    let started = Instant::now();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut out_done = false;
    let mut err_done = false;
    loop {
        if started.elapsed() >= timeout {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let mut progress = false;
        for (pipe, bytes, done) in [
            (&mut stdout as &mut dyn Read, &mut out, &mut out_done),
            (&mut stderr as &mut dyn Read, &mut err, &mut err_done),
        ] {
            if *done {
                continue;
            }
            let mut chunk = [0; 8192];
            match pipe.read(&mut chunk) {
                Ok(0) => *done = true,
                Ok(count) => {
                    if bytes.len() + count > OUTPUT_LIMIT {
                        return Err(io::Error::other("metadata output limit"));
                    }
                    bytes.extend_from_slice(&chunk[..count]);
                    progress = true;
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(error),
            }
        }
        if let Some(status) = child.0.try_wait()? {
            if out_done && err_done {
                return Ok(Output {
                    status,
                    stdout: out,
                    stderr: err,
                });
            }
        }
        if !progress {
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

#[cfg(not(unix))]
pub fn collect(child: Child, _: Duration) -> io::Result<Output> {
    let _guard = ChildGuard(child);
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    fn child(script: &str) -> Child {
        Command::new("python3")
            .args(["-c", script])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }
    #[test]
    fn drains_both_pipes_before_exit() {
        let output = collect(child("import sys; sys.stderr.write('e'*60000); sys.stderr.flush(); sys.stdout.write('o'*60000)"), Duration::from_secs(3)).unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout.len(), 60000);
        assert_eq!(output.stderr.len(), 60000);
    }
    #[test]
    fn rejects_flood_and_hung_child() {
        assert!(
            collect(
                child("import sys; sys.stdout.write('x'*100000)"),
                Duration::from_secs(3)
            )
            .is_err()
        );
        assert_eq!(
            collect(
                child("import time; time.sleep(30)"),
                Duration::from_millis(100)
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::TimedOut
        );
    }
}
