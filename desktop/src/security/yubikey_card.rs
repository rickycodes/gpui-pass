use super::*;

pub(crate) fn check_card_status(executable: &std::path::Path, probe_id: u64) -> CardProbeResult {
    trace!("probe.query id={probe_id} mode=card-status-all");
    let Ok(child) = Command::new(executable)
        .args([
            "--batch",
            "--no-tty",
            "--with-colons",
            "--status-fd=2",
            // Plain card-status only LEARNs the current session. The explicit
            // all form issues SERIALNO first, rescanning readers after replug.
            "--card-status",
            "all",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    else {
        return CardProbeResult {
            connection: YubiConnection::Unavailable,
            details: None,
        };
    };

    trace!("probe.spawned id={probe_id} child_pid={}", child.id());
    let started = Instant::now();
    let output = match process_output::collect(child, Duration::from_secs(5)) {
        Ok(output) => output,
        Err(_) => {
            return CardProbeResult {
                connection: YubiConnection::Busy,
                details: None,
            };
        }
    };
    trace!(
        "probe.output id={probe_id} stdout_bytes={} stderr_bytes={} elapsed_ms={}",
        output.stdout.len(),
        output.stderr.len(),
        started.elapsed().as_millis()
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined_output = format!("{stdout}\n{stderr}");
    match parse_card_status(&combined_output) {
        ParsedCardStatus::Detected(details) => {
            return CardProbeResult {
                connection: YubiConnection::Connected,
                details: Some(details),
            };
        }
        ParsedCardStatus::NotDetected if output.status.success() => {
            return CardProbeResult {
                connection: YubiConnection::NotDetected,
                details: None,
            };
        }
        // The all form can successfully return no records when no card was found.
        ParsedCardStatus::Unrecognized if output.status.success() => {
            return CardProbeResult {
                connection: YubiConnection::NotDetected,
                details: None,
            };
        }
        ParsedCardStatus::NotDetected | ParsedCardStatus::Unrecognized => {}
    }

    // Diagnostics are consulted only to classify failures and are never retained or displayed.
    for line in stderr.lines() {
        for status in ["ERROR", "FAILURE", "SC_OP_FAILURE"] {
            if line
                .strip_prefix("[GNUPG:] ")
                .and_then(|line| line.strip_prefix(status))
                .is_some_and(|rest| rest.starts_with(' '))
            {
                let code = line
                    .split_whitespace()
                    .last()
                    .and_then(|value| value.parse::<u32>().ok());
                trace!("probe.gpg_error id={probe_id} status={status} code={code:?}");
            }
        }
    }
    let diagnostic = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    let connection = if diagnostic.contains("no agent running")
        || diagnostic.contains("can't connect to the agent")
        || diagnostic.contains("cannot connect to the agent")
    {
        YubiConnection::Unavailable
    } else if diagnostic.contains("busy") || diagnostic.contains("locked") {
        YubiConnection::Busy
    } else if diagnostic.contains("no reader") || diagnostic.contains("reader not found") {
        YubiConnection::NoReader
    } else if [
        "no such device",
        "no card",
        "card not present",
        "selecting card failed",
        "card not available",
    ]
    .iter()
    .any(|message| diagnostic.contains(message))
    {
        YubiConnection::NotDetected
    } else {
        YubiConnection::Unavailable
    };
    CardProbeResult {
        connection,
        details: None,
    }
}
