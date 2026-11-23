use super::*;

pub(crate) fn parse_card_status(output: &str) -> ParsedCardStatus {
    // card-status all may return several cards. Select the first available
    // record; never combine one reader's version with another card's PIN data.
    let mut start = 0;
    let mut offset = 0;
    let mut reader_seen = false;
    for line in output.split_inclusive('\n') {
        let label = line
            .split_once(':')
            .map(|(label, _)| label.trim().trim_matches('.').trim());
        if label.is_some_and(|label| label.eq_ignore_ascii_case("reader")) {
            if reader_seen {
                if let ParsedCardStatus::Detected(details) =
                    parse_one_card_status(&output[start..offset])
                {
                    return ParsedCardStatus::Detected(details);
                }
                start = offset;
            }
            reader_seen = true;
        }
        offset += line.len();
    }
    parse_one_card_status(&output[start..])
}

pub(crate) fn parse_one_card_status(output: &str) -> ParsedCardStatus {
    let mut application = None;
    let mut reader = None;
    let mut version = None;
    let mut manufacturer = None;
    let mut key_attributes = Vec::new();
    let mut pin_retries = None;
    let mut signature_count = None;
    let mut aid_record_seen = false;
    let mut aid_present = false;

    for line in output.lines() {
        let fields: Vec<_> = line.split(':').collect();
        let (raw_label, raw_value) = line.split_once(':').unwrap_or((line, ""));
        let label = raw_label
            .trim()
            .trim_matches('.')
            .trim()
            .to_ascii_lowercase();
        let value = raw_value.trim().trim_end_matches(':').trim();
        match label.as_str() {
            "reader" => reader = Some(safe_card_text(value)),
            "aid" => {
                aid_record_seen = true;
                aid_present = fields.get(1).is_some_and(|value| !value.is_empty());
            }
            "openpgp-card" => application = Some("OpenPGP".to_owned()),
            "piv-card" => application = Some("PIV".to_owned()),
            "netkey-card" => application = Some("NetKey".to_owned()),
            "dinsig-card" => application = Some("DINSIG".to_owned()),
            "pkcs15-card" => application = Some("PKCS#15".to_owned()),
            "geldkarte-card" => application = Some("Geldkarte".to_owned()),
            "unknown-card" => application = Some("Unknown card application".to_owned()),
            tag if tag.ends_with("-card") => {
                application = Some(
                    tag.trim_end_matches("-card")
                        .replace('-', " ")
                        .to_ascii_uppercase(),
                );
            }
            "application type" => {
                if !value.is_empty() {
                    application = Some(safe_card_text(value));
                }
            }
            "version" => {
                version = decode_card_version(if value.is_empty() {
                    fields.get(1).copied().unwrap_or_default()
                } else {
                    value
                });
            }
            "vendor" => {
                manufacturer = fields.get(2).map(|value| safe_card_text(value));
            }
            "manufacturer" => manufacturer = Some(safe_card_text(value)),
            "keyattr" => {
                if let (Some(slot), Some(algorithm), Some(parameter)) =
                    (fields.get(1), fields.get(2), fields.get(3))
                    && let Some(attribute) = card_key_attribute(slot, algorithm, parameter)
                {
                    key_attributes.push(attribute);
                }
            }
            "key attributes" => {
                key_attributes.extend(
                    value
                        .split_whitespace()
                        .map(|attribute| format!("Key: {}", safe_card_text(attribute))),
                );
            }
            "pinretry" => {
                if let (Some(first), Some(second), Some(third)) = (
                    fields.get(1).and_then(|value| value.parse().ok()),
                    fields.get(2).and_then(|value| value.parse().ok()),
                    fields.get(3).and_then(|value| value.parse().ok()),
                ) {
                    pin_retries = Some([first, second, third]);
                }
            }
            "pin retry counter" => {
                let retries: Vec<_> = value
                    .split_whitespace()
                    .filter_map(|value| value.parse().ok())
                    .collect();
                if let [first, second, third] = retries.as_slice() {
                    pin_retries = Some([*first, *second, *third]);
                }
            }
            "sigcount" => signature_count = fields.get(1).and_then(|value| value.parse().ok()),
            "signature counter" => signature_count = value.parse().ok(),
            _ => {}
        }
    }

    if aid_present
        || application.is_some()
        || reader.as_ref().is_some_and(|value| !value.is_empty())
        || version.is_some()
        || manufacturer.is_some()
        || !key_attributes.is_empty()
        || pin_retries.is_some()
        || signature_count.is_some()
    {
        ParsedCardStatus::Detected(CardDetails {
            application: application.unwrap_or_else(|| "Smartcard".to_owned()),
            reader: reader.filter(|value| !value.is_empty()),
            version,
            manufacturer: manufacturer.filter(|value| !value.is_empty()),
            key_attributes,
            pin_retries,
            signature_count,
        })
    } else if aid_record_seen {
        ParsedCardStatus::NotDetected
    } else {
        ParsedCardStatus::Unrecognized
    }
}

pub(crate) fn card_key_attribute(slot: &str, algorithm: &str, parameter: &str) -> Option<String> {
    let slot = match slot {
        "1" => "Signing",
        "2" => "Encryption",
        "3" => "Authentication",
        _ => return None,
    };
    let algorithm = match algorithm {
        "1" => format!("RSA {parameter}"),
        "18" => format!("ECDH {}", safe_card_text(parameter)),
        "19" => format!("ECDSA {}", safe_card_text(parameter)),
        "22" => format!("EdDSA {}", safe_card_text(parameter)),
        value => format!("algorithm {value}"),
    };
    Some(format!("{slot}: {algorithm}"))
}

pub(crate) fn decode_card_version(value: &str) -> Option<String> {
    if value.len() == 4 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        let major = u8::from_str_radix(&value[..2], 16).ok()?;
        let minor = u8::from_str_radix(&value[2..], 16).ok()?;
        Some(format!("{major}.{minor}"))
    } else if value.is_empty() {
        None
    } else {
        Some(safe_card_text(value))
    }
}

pub(crate) fn safe_card_text(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_digit(bytes[index + 1]), hex_digit(bytes[index + 2]))
        {
            decoded.push(high * 16 + low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&decoded)
        .chars()
        .filter(|character| !character.is_control())
        .collect()
}

pub(crate) fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}
