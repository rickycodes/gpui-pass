use std::{collections::BTreeMap, path::Path, process::Command};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RecipientChoice {
    pub(crate) label: String,
    pub(crate) email: Option<String>,
    pub(crate) fingerprint: String,
}

struct SecretKey {
    valid: bool,
    fingerprint: Option<String>,
    user_ids: Vec<String>,
}

pub(crate) fn discover_recipients(executable: &Path) -> Vec<RecipientChoice> {
    let Ok(output) = Command::new(executable)
        .args([
            "--no-options",
            "--batch",
            "--no-tty",
            "--with-colons",
            "--fingerprint",
            "--list-secret-keys",
        ])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let Ok(output) = std::str::from_utf8(&output.stdout) else {
        return Vec::new();
    };
    parse_recipient_choices(output)
}

pub(crate) fn single_email_suggestion(choices: &[RecipientChoice]) -> Option<&str> {
    if choices.len() == 1 {
        choices[0].email.as_deref()
    } else {
        None
    }
}

fn parse_recipient_choices(output: &str) -> Vec<RecipientChoice> {
    let mut keys = Vec::new();
    let mut current: Option<SecretKey> = None;

    for line in output.lines() {
        let fields: Vec<_> = line.split(':').collect();
        match fields.first().copied() {
            Some("sec" | "sec>") => {
                if let Some(key) = current.take() {
                    keys.push(key);
                }
                current = Some(SecretKey {
                    valid: valid_status(fields.get(1).copied()),
                    fingerprint: None,
                    user_ids: Vec::new(),
                });
            }
            Some("fpr") => {
                if let Some(key) = current.as_mut()
                    && key.fingerprint.is_none()
                {
                    key.fingerprint = fields.get(9).and_then(|field| fingerprint(field));
                }
            }
            Some("uid") => {
                let Some(key) = current.as_mut() else {
                    continue;
                };
                if key.valid && valid_status(fields.get(1).copied()) {
                    if let Some(uid) = fields.get(9).and_then(|field| decode_colon_field(field)) {
                        let label: String = uid.chars().filter(|char| !char.is_control()).collect();
                        if !label.is_empty() {
                            key.user_ids.push(label);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(key) = current {
        keys.push(key);
    }

    keys.into_iter()
        .filter(|key| key.valid)
        .flat_map(SecretKey::into_choices)
        .collect()
}

impl SecretKey {
    fn into_choices(self) -> Vec<RecipientChoice> {
        let Some(fingerprint) = self.fingerprint else {
            return Vec::new();
        };
        let mut emails = BTreeMap::new();
        for uid in &self.user_ids {
            if let Some(email) = email_from_uid(uid) {
                emails.entry(email).or_insert_with(|| uid.clone());
            }
        }

        if emails.is_empty() {
            return vec![RecipientChoice {
                label: self
                    .user_ids
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| format!("GPG key {fingerprint}")),
                email: None,
                fingerprint,
            }];
        }

        emails
            .into_iter()
            .map(|(email, label)| RecipientChoice {
                label,
                email: Some(email),
                fingerprint: fingerprint.clone(),
            })
            .collect()
    }
}

fn valid_status(status: Option<&str>) -> bool {
    status.is_some_and(|status| matches!(status, "-" | "q" | "n" | "m" | "f" | "u"))
}

fn fingerprint(field: &str) -> Option<String> {
    matches!(field.len(), 40 | 64)
        .then(|| field.to_owned())
        .filter(|field| field.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn email_from_uid(uid: &str) -> Option<String> {
    let email = if let Some(start) = uid.rfind('<') {
        let end = uid[start + 1..].find('>')? + start + 1;
        &uid[start + 1..end]
    } else if uid.contains('@') && !uid.chars().any(char::is_whitespace) {
        uid
    } else {
        return None;
    };
    let (local, domain) = email.split_once('@')?;
    (!local.is_empty() && !domain.is_empty()).then(|| email.to_owned())
}

fn decode_colon_field(field: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(field.len());
    let mut chars = field.bytes();
    while let Some(byte) = chars.next() {
        if byte == b'%' {
            let high = hex_value(chars.next()?)?;
            let low = hex_value(chars.next()?)?;
            bytes.push((high << 4) | low);
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8(bytes).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST_FINGERPRINT: &str = "0123456789ABCDEF0123456789ABCDEF01234567";
    const SECOND_FINGERPRINT: &str = "89ABCDEF0123456789ABCDEF0123456789ABCDEF";

    fn key_record(kind: &str, validity: &str, fingerprint: &str) -> String {
        format!(
            "{}\n{}",
            [kind, validity, "", "", "", "", "", "", "", "", "", ""].join(":"),
            fingerprint_record(fingerprint)
        )
    }

    fn fingerprint_record(fingerprint: &str) -> String {
        let mut fields = [""; 10];
        fields[0] = "fpr";
        fields[9] = fingerprint;
        fields.join(":")
    }

    fn uid_record(validity: &str, uid: &str) -> String {
        let mut fields = [""; 10];
        fields[0] = "uid";
        fields[1] = validity;
        fields[9] = uid;
        fields.join(":")
    }

    #[test]
    fn suggests_email_for_one_unambiguous_secret_key() {
        let output = format!(
            "{}\n{}\n",
            key_record("sec", "u", FIRST_FINGERPRINT),
            uid_record("u", "Alice Example <alice@example.com>")
        );
        let choices = parse_recipient_choices(&output);
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].email.as_deref(), Some("alice@example.com"));
        assert_eq!(choices[0].fingerprint, FIRST_FINGERPRINT);
        assert_eq!(single_email_suggestion(&choices), Some("alice@example.com"));
    }

    #[test]
    fn returns_distinct_choices_for_multiple_keys_or_emails() {
        let multiple_keys = format!(
            "{}\n{}\n{}\n{}\n",
            key_record("sec", "u", FIRST_FINGERPRINT),
            uid_record("u", "Alice <alice@example.com>"),
            key_record("sec", "u", SECOND_FINGERPRINT),
            uid_record("u", "Bob <bob@example.com>")
        );
        let choices = parse_recipient_choices(&multiple_keys);
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].fingerprint, FIRST_FINGERPRINT);
        assert_eq!(choices[1].fingerprint, SECOND_FINGERPRINT);
        assert_eq!(single_email_suggestion(&choices), None);

        let multiple_emails = format!(
            "{}\n{}\n{}\n",
            key_record("sec", "u", FIRST_FINGERPRINT),
            uid_record("u", "Alice <alice@example.com>"),
            uid_record("u", "Alice <other@example.com>")
        );
        assert_eq!(parse_recipient_choices(&multiple_emails).len(), 2);
        let shared_email = format!(
            "{}\n{}\n{}\n{}\n",
            key_record("sec", "u", FIRST_FINGERPRINT),
            uid_record("u", "Alice <alice@example.com>"),
            key_record("sec", "u", SECOND_FINGERPRINT),
            uid_record("u", "Alice <alice@example.com>")
        );
        let choices = parse_recipient_choices(&shared_email);
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].email, choices[1].email);
        assert_ne!(choices[0].fingerprint, choices[1].fingerprint);
    }

    #[test]
    fn leaves_empty_and_unusable_keyrings_without_suggestions() {
        assert!(parse_recipient_choices("").is_empty());
        let unavailable = format!(
            "{}\n{}\n",
            key_record("sec#", "u", FIRST_FINGERPRINT),
            uid_record("u", "Alice <alice@example.com>")
        );
        assert!(parse_recipient_choices(&unavailable).is_empty());
        let expired = format!(
            "{}\n{}\n",
            key_record("sec", "e", FIRST_FINGERPRINT),
            uid_record("u", "Alice <alice@example.com>")
        );
        assert!(parse_recipient_choices(&expired).is_empty());
    }

    #[test]
    fn supports_keys_without_email_uids_and_decodes_gpg_escapes() {
        let output = format!(
            "{}\n{}\n",
            key_record("sec", "u", FIRST_FINGERPRINT),
            uid_record("u", "Alice Example")
        );
        let choices = parse_recipient_choices(&output);
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].email, None);
        assert_eq!(choices[0].label, "Alice Example");
        assert_eq!(
            email_from_uid(&decode_colon_field("Alice%20%3Calice%40example.com%3E").unwrap())
                .as_deref(),
            Some("alice@example.com")
        );
    }
}
