//! Allowlisted GnuPG status decoding. Raw diagnostic text never leaves this module.
use crate::{OperationProgress, OperationStatus};
use std::io::{self, Read};
use zeroize::Zeroizing;

pub(crate) const UNLOCKING: OperationStatus = OperationStatus::working(
    "Unlocking entry",
    "Your entry stays hidden while your key is being unlocked.",
);
pub(crate) const CHECK_KEY: OperationStatus = OperationStatus::attention(
    "Check your YubiKey or system prompt",
    "If your YubiKey is blinking, touch its gold contact. Complete any open PIN or passphrase prompt first.",
);
const PINENTRY: OperationStatus = OperationStatus::attention(
    "Complete the system prompt",
    "Follow the system prompt to unlock your key. If your YubiKey then blinks, touch its gold contact to continue.",
);
const CARD_BUSY: OperationStatus = OperationStatus::attention(
    "Check your YubiKey",
    "Your security key is busy. If your YubiKey is blinking, touch its gold contact to continue.",
);
const INSERT_CARD: OperationStatus = OperationStatus::attention(
    "Connect your security key",
    "Insert the YubiKey or smartcard that holds this entry's key, then follow the system prompt.",
);
const AGENT_BUSY: OperationStatus = OperationStatus::working(
    "Waiting for another operation",
    "Another application is using your security key. Finish its request to continue here.",
);
const DECRYPTING: OperationStatus = OperationStatus::working(
    "Decrypting entry",
    "Your entry stays hidden until decryption and integrity checks succeed.",
);

/// Fixed storage, bounded lines, and bounded reads per poll preserve cancellation responsiveness.
pub(crate) struct StatusReader {
    line: Zeroizing<[u8; 4096]>,
    used: usize,
    discard_line: bool,
    scratch: Zeroizing<[u8; 4096]>,
}

impl StatusReader {
    pub(crate) fn new() -> Self {
        Self {
            line: Zeroizing::new([0; 4096]),
            used: 0,
            discard_line: false,
            scratch: Zeroizing::new([0; 4096]),
        }
    }

    pub(crate) fn drain(
        &mut self,
        input: &mut impl Read,
        progress: &OperationProgress,
    ) -> io::Result<()> {
        for _ in 0..4 {
            match input.read(&mut *self.scratch) {
                Ok(0) => break,
                Ok(count) => {
                    // Process bytewise so no second copy of diagnostics is allocated.
                    for index in 0..count {
                        let byte = self.scratch[index];
                        self.push(byte, progress);
                        self.scratch[index] = 0;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn push(&mut self, byte: u8, progress: &OperationProgress) {
        if byte == b'\n' {
            if !self.discard_line {
                if let Some(status) = decode(&self.line[..self.used]) {
                    progress.update(status);
                }
            }
            self.line[..self.used].fill(0);
            self.used = 0;
            self.discard_line = false;
        } else if !self.discard_line {
            if self.used == self.line.len() {
                self.line.fill(0);
                self.used = 0;
                self.discard_line = true;
            } else {
                self.line[self.used] = byte;
                self.used += 1;
            }
        }
    }
}

fn decode(line: &[u8]) -> Option<OperationStatus> {
    let payload = line.strip_prefix(b"[GNUPG:] ")?;
    let mut fields = payload
        .split(|byte| byte.is_ascii_whitespace())
        .filter(|part| !part.is_empty());
    match fields.next()? {
        b"PINENTRY_LAUNCHED" if fields.next().is_some() => Some(PINENTRY),
        b"CARDCTRL" => match fields.next()? {
            b"1" | b"4" | b"5" => Some(INSERT_CARD),
            b"3" => Some(UNLOCKING),
            _ => None,
        },
        b"PROGRESS" => {
            let operation = fields.next()?;
            let character = fields.next()?;
            let current = fields.next()?;
            let total = fields.next()?;
            if character.len() != 1 || !is_number(current) || !is_number(total) {
                return None;
            }
            match operation {
                b"card_busy" => Some(CARD_BUSY),
                b"scd_locked" => Some(AGENT_BUSY),
                _ => None,
            }
        }
        b"BEGIN_DECRYPTION" | b"DECRYPTION_OKAY" => Some(DECRYPTING),
        _ => None,
    }
}

fn is_number(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(u8::is_ascii_digit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_diagnostics_metadata_and_malformed_status_lines() {
        for line in [
            &b"secret diagnostic text"[..],
            b"[GNUPG:] USERID_HINT fingerprint private@example.test",
            b"[GNUPG:] PLAINTEXT 62 0 private-filename",
            b"[GNUPG:] PROGRESS card_busy",
            b"[GNUPG:] PROGRESS card_busy w invalid 0",
            b"[GNUPG:] PINENTRY_LAUNCHED",
            b"prefix [GNUPG:] PROGRESS card_busy w 0 0",
        ] {
            assert!(decode(line).is_none());
        }
    }

    #[test]
    fn handles_split_lines_and_drops_entire_oversized_lines() {
        let progress = OperationProgress::new(UNLOCKING);
        let mut reader = StatusReader::new();
        for &byte in b"[GNUPG:] PINENTRY_" {
            reader.push(byte, &progress);
        }
        assert_eq!(progress.snapshot(), UNLOCKING);
        for &byte in b"LAUNCHED 123 metadata-to-ignore\r\n" {
            reader.push(byte, &progress);
        }
        assert_eq!(progress.snapshot(), PINENTRY);
        for _ in 0..8192 {
            reader.push(b'x', &progress);
        }
        for &byte in b"[GNUPG:] PROGRESS card_busy w 0 0\n" {
            reader.push(byte, &progress);
        }
        assert_eq!(progress.snapshot(), PINENTRY);
        assert_eq!(reader.used, 0);
        assert!(reader.line.iter().all(|byte| *byte == 0));
        for &byte in b"[GNUPG:] PROGRESS card_busy w 0 0\n" {
            reader.push(byte, &progress);
        }
        assert_eq!(progress.snapshot(), CARD_BUSY);
        for &byte in b"[GNUPG:] CARDCTRL 1 ignored-serial\n" {
            reader.push(byte, &progress);
        }
        assert_eq!(progress.snapshot(), INSERT_CARD);
        for &byte in b"[GNUPG:] CARDCTRL 3 ignored-serial\n" {
            reader.push(byte, &progress);
        }
        assert_eq!(progress.snapshot(), UNLOCKING);
    }
}
