//! Detects content IGRIS must never store in memory: credentials, keys,
//! payment cards and government ID numbers. Heuristic by design — it errs on
//! the side of refusing.

use std::sync::OnceLock;

use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensitiveKind {
    Password,
    ApiKey,
    PrivateKey,
    PaymentCard,
    GovernmentId,
}

impl SensitiveKind {
    pub fn describe(self) -> &'static str {
        match self {
            SensitiveKind::Password => "a password or PIN",
            SensitiveKind::ApiKey => "an API key or access token",
            SensitiveKind::PrivateKey => "a private key",
            SensitiveKind::PaymentCard => "a payment card number",
            SensitiveKind::GovernmentId => "a government ID number",
        }
    }
}

fn patterns() -> &'static [(SensitiveKind, Regex)] {
    static P: OnceLock<Vec<(SensitiveKind, Regex)>> = OnceLock::new();
    P.get_or_init(|| {
        let r = |s: &str| Regex::new(s).expect("valid regex");
        vec![
            (SensitiveKind::PrivateKey, r(r"-----BEGIN [A-Z ]*PRIVATE KEY-----")),
            (
                SensitiveKind::ApiKey,
                r(r"\b(sk-[A-Za-z0-9_-]{16,}|sk-ant-[A-Za-z0-9_-]{10,}|AKIA[0-9A-Z]{16}|gh[pousr]_[A-Za-z0-9]{20,}|xox[abprs]-[A-Za-z0-9-]{10,}|AIza[0-9A-Za-z_-]{30,}|eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]+)"),
            ),
            (
                SensitiveKind::ApiKey,
                r(r"(?i)\b(api[ _-]?key|secret[ _-]?key|access[ _-]?token|auth[ _-]?token|bearer)\b\s*(is|:|=)\s*\S{8,}"),
            ),
            (
                SensitiveKind::Password,
                r(r"(?i)\b(password|passwd|pwd|passcode|passphrase)\b\s*(is|was|:|=|-)?\s*\S+"),
            ),
            (SensitiveKind::Password, r(r"(?i)\b(pin|otp)\b\s*(code\s*)?(is|:|=)\s*\d{4,8}\b")),
            // US SSN, Indian Aadhaar / PAN.
            (SensitiveKind::GovernmentId, r(r"\b\d{3}-\d{2}-\d{4}\b")),
            (SensitiveKind::GovernmentId, r(r"(?i)\baadhaar\b.*\d{4}\s?\d{4}\s?\d{4}|\b[2-9]\d{3}\s\d{4}\s\d{4}\b")),
            (SensitiveKind::GovernmentId, r(r"\b[A-Z]{5}\d{4}[A-Z]\b")),
        ]
    })
}

/// Luhn check for candidate card numbers (13–19 digits, spaces/dashes allowed).
fn contains_card_number(text: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    let r = R.get_or_init(|| Regex::new(r"\b(?:\d[ -]?){13,19}\b").expect("valid regex"));
    r.find_iter(text).any(|m| {
        let digits: Vec<u32> = m.as_str().chars().filter_map(|c| c.to_digit(10)).collect();
        if !(13..=19).contains(&digits.len()) {
            return false;
        }
        let sum: u32 = digits
            .iter()
            .rev()
            .enumerate()
            .map(|(i, &d)| if i % 2 == 1 { let x = d * 2; if x > 9 { x - 9 } else { x } } else { d })
            .sum();
        sum % 10 == 0
    })
}

pub fn detect(text: &str) -> Option<SensitiveKind> {
    if contains_card_number(text) {
        return Some(SensitiveKind::PaymentCard);
    }
    patterns().iter().find(|(_, re)| re.is_match(text)).map(|(k, _)| *k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_secrets() {
        let cases = [
            ("my password is hunter2", SensitiveKind::Password),
            ("Password: Tr0ub4dor&3", SensitiveKind::Password),
            ("my bank PIN is 4821", SensitiveKind::Password),
            ("key sk-ant-api03-abcdefghijklmnop", SensitiveKind::ApiKey),
            ("aws AKIAIOSFODNN7EXAMPLE", SensitiveKind::ApiKey),
            ("token ghp_abcdefghijklmnopqrstuvwxyz0123", SensitiveKind::ApiKey),
            ("my api key is 9f8e7d6c5b4a3210", SensitiveKind::ApiKey),
            ("-----BEGIN RSA PRIVATE KEY-----", SensitiveKind::PrivateKey),
            ("card 4111 1111 1111 1111", SensitiveKind::PaymentCard),
            ("4111-1111-1111-1111 exp 12/29", SensitiveKind::PaymentCard),
            ("SSN 123-45-6789", SensitiveKind::GovernmentId),
            ("my aadhaar is 2345 6789 0123", SensitiveKind::GovernmentId),
            ("PAN ABCDE1234F", SensitiveKind::GovernmentId),
        ];
        for (text, kind) in cases {
            assert_eq!(detect(text), Some(kind), "{text}");
        }
    }

    #[test]
    fn allows_ordinary_facts() {
        for text in [
            "My main project is called SkillTrack",
            "I prefer TypeScript over JavaScript",
            "My phone number area code is 0124",
            "The build takes about 1234567 ms on CI",
            "I work at MRU as director of IQAC",
            "Remind me that the meeting PIN code changes weekly",
            "Order number 1234567890123 shipped",
        ] {
            assert_eq!(detect(text), None, "{text}");
        }
    }
}
