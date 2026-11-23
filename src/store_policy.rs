use super::*;

pub(crate) const GPG_ID_FILE: &str = ".gpg-id";
pub(crate) const GPG_ID_SIGNATURE_FILE: &str = ".gpg-id.sig";
pub(crate) const MAX_RECIPIENTS_BYTES: usize = 65_536;

pub(crate) fn parse_recipients(text: &str) -> Result<Vec<String>, Error> {
    let mut recipients = Vec::new();
    for line in text.lines() {
        for recipient in line
            .split('#')
            .next()
            .unwrap_or_default()
            .split_whitespace()
        {
            if recipient.len() > 256 || recipient.chars().any(char::is_control) {
                return Err(Error::InvalidRecipients);
            }
            if !recipients.iter().any(|existing| existing == recipient) {
                recipients.push(recipient.to_owned());
                if recipients.len() > 256 {
                    return Err(Error::InvalidRecipients);
                }
            }
        }
    }
    if recipients.is_empty() {
        Err(Error::MissingRecipients)
    } else {
        Ok(recipients)
    }
}

pub fn configured_signing_keys() -> Result<Vec<String>, Error> {
    let Some(value) = env::var_os("PASSWORD_STORE_SIGNING_KEY").filter(|value| !value.is_empty())
    else {
        return Ok(Vec::new());
    };
    let keys: Vec<_> = value
        .to_string_lossy()
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    if keys.is_empty()
        || keys.iter().any(|key| {
            key.len() != 40
                || !key
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte))
        })
    {
        return Err(Error::InvalidSigningKey);
    }
    Ok(keys)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RecipientPolicy {
    pub(crate) recipients: Vec<String>,
    pub(crate) contents: Option<Vec<u8>>,
    pub(crate) signature: Option<Vec<u8>>,
    pub(crate) signing_keys: Vec<String>,
}

pub fn default_store_path() -> Result<PathBuf, Error> {
    if let Some(path) = env::var_os("PASSWORD_STORE_DIR").filter(|v| !v.is_empty()) {
        return Ok(path.into());
    }
    env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(|home| PathBuf::from(home).join(".password-store"))
        .ok_or(Error::MissingHome)
}
