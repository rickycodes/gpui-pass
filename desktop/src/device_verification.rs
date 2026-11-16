use crate::diagnostics::trace;
use std::{
    io::{Read, Write},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum State {
    #[default]
    Idle,
    Pending,
    Verified,
    Untrusted,
    Unsupported,
    Unavailable,
    Multiple,
    TimedOut,
    Failed,
    Cancelled,
    Busy,
}

impl State {
    pub fn message(self) -> &'static str {
        match self {
            Self::Idle => "",
            Self::Pending => "Touch your YubiKey to verify it.",
            Self::Verified => "Verified — trusted Yubico device attestation.",
            Self::Untrusted => {
                "Could not verify this device’s attestation against the bundled Yubico certificates."
            }
            Self::Unsupported => "This device does not support the FIDO U2F attestation check.",
            Self::Unavailable => {
                "Verification needs FIDO HID access, Python fido2, and OpenSSL 3 on Linux."
            }
            Self::Multiple => {
                "Multiple matching keys found. Connect only the device you want to verify."
            }
            Self::TimedOut => {
                "Verification timed out. Try again and touch your YubiKey when it blinks."
            }
            Self::Failed => {
                "Verification could not complete. Check the device connection and try again."
            }
            Self::Cancelled => "Verification cancelled.",
            Self::Busy => "Wait for the current device operation to finish, then try again.",
        }
    }
}

struct Task {
    generation: u64,
    cancellation: Arc<AtomicBool>,
    receiver: Receiver<State>,
}

impl Drop for Task {
    fn drop(&mut self) {
        self.cancellation.store(true, Ordering::Release);
    }
}

#[derive(Default)]
pub(crate) struct DeviceVerification {
    state: State,
    task: Option<Task>,
}

impl DeviceVerification {
    pub fn state(&self) -> State {
        self.state
    }
    pub fn running(&self) -> bool {
        self.task.is_some()
    }
    pub fn unavailable(&mut self) {
        self.state = State::Unavailable;
    }
    pub fn busy(&mut self) {
        self.state = State::Busy;
    }

    pub fn start(&mut self, generation: u64, product_id: u16) {
        if self.running() {
            return;
        }
        let cancellation = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancellation.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        self.state = State::Pending;
        self.task = Some(Task {
            generation,
            cancellation,
            receiver,
        });
        trace!("verification.start generation={generation} transport=fido-hid");
        std::thread::spawn(move || {
            let state = run_helper(product_id, &worker_cancel);
            trace!("verification.worker_complete generation={generation} result={state:?}");
            let _ = sender.send(state);
        });
    }

    pub fn cancel(&mut self) {
        if let Some(task) = &self.task {
            task.cancellation.store(true, Ordering::Release);
            self.state = State::Cancelled;
        }
    }

    pub fn invalidate(&mut self) {
        self.cancel();
        self.state = State::Idle;
    }

    pub fn poll(&mut self, generation: u64) -> bool {
        let Some(task) = &self.task else {
            return false;
        };
        if task.generation != generation {
            task.cancellation.store(true, Ordering::Release);
        }
        let result = match task.receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => State::Failed,
        };
        let accepted = task.generation == generation;
        self.state = if !accepted {
            State::Idle
        } else if task.cancellation.load(Ordering::Acquire) {
            State::Cancelled
        } else {
            result
        };
        trace!(
            "verification.complete generation={} accepted={accepted} result={:?}",
            task.generation, self.state
        );
        self.task = None;
        true
    }
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run_helper(product_id: u16, cancel: &AtomicBool) -> State {
    let started = Instant::now();
    let Ok(child) = Command::new("python3")
        .args([
            "-c",
            include_str!("verify_device.py"),
            &product_id.to_string(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return State::Unavailable;
    };
    let mut child = ChildGuard(child);
    let Some(mut input) = child.0.stdin.take() else {
        return State::Failed;
    };
    // Static public certificates only. The helper consumes this before touching HID.
    if input
        .write_all(include_bytes!("../../assets/yubico-fido-roots.pem"))
        .is_err()
        || input.write_all(b"\n---INTERMEDIATES---\n").is_err()
        || input
            .write_all(include_bytes!("../../assets/yubico-intermediates.pem"))
            .is_err()
    {
        return State::Failed;
    }
    drop(input);
    loop {
        if cancel.load(Ordering::Acquire) {
            return State::Cancelled;
        }
        if started.elapsed() >= Duration::from_secs(65) {
            return State::TimedOut;
        }
        match child.0.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return State::Failed;
                }
                let Some(output) = child.0.stdout.take() else {
                    return State::Failed;
                };
                let mut token = String::new();
                if output.take(128).read_to_string(&mut token).is_err() {
                    return State::Failed;
                }
                return parse_result(&token);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => return State::Failed,
        }
    }
}

fn parse_result(token: &str) -> State {
    match token.trim() {
        "verified" => State::Verified,
        "untrusted" => State::Untrusted,
        "unsupported" => State::Unsupported,
        "unavailable" => State::Unavailable,
        "multiple" => State::Multiple,
        "timeout" => State::TimedOut,
        _ => State::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_or_partial_responses_never_verify() {
        for text in [
            "",
            "verified\nfailed",
            "true",
            "VERIFIED",
            "verified but untrusted",
        ] {
            assert_eq!(parse_result(text), State::Failed);
        }
        assert_eq!(parse_result("verified\n"), State::Verified);
    }

    #[test]
    fn unplugged_or_cancelled_results_never_verify_a_new_insertion() {
        for (new_generation, cancelled, expected) in
            [(2, false, State::Idle), (1, true, State::Cancelled)]
        {
            let (sender, receiver) = mpsc::channel();
            let cancellation = Arc::new(AtomicBool::new(cancelled));
            let mut verification = DeviceVerification {
                state: State::Pending,
                task: Some(Task {
                    generation: 1,
                    cancellation,
                    receiver,
                }),
            };
            sender.send(State::Verified).unwrap();
            assert!(verification.poll(new_generation));
            assert_eq!(verification.state(), expected);
            assert!(!verification.running());
        }
    }
}
