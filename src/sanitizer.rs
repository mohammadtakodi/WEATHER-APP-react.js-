use regex::Regex;
use std::borrow::Cow;
use lazy_static::lazy_static;

lazy_static! {
    static ref EMAIL_REGEX: Regex = Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap();
    static ref PHONE_REGEX: Regex = Regex::new(r"(\+\d{1,2}\s)?\(?\d{3}\)?[\s.-]\d{3}[\s.-]\d{4}").unwrap();
    static ref CREDIT_CARD_REGEX: Regex = Regex::new(r"\b(?:\d[ -]*?){13,16}\b").unwrap();
    // Simple Name Entity Recognition is hard without a model, so we'll skip complex names for now
    // In production, use `rust-bert` or similar
}

pub struct Sanitizer;

impl Sanitizer {
    pub fn sanitize(text: &str) -> String {
        let mut sanitized = text.to_string();

        // 1. Mask Emails
        sanitized = EMAIL_REGEX.replace_all(&sanitized, "[EMAIL_REDACTED]").to_string();

        // 2. Mask Phone Numbers
        sanitized = PHONE_REGEX.replace_all(&sanitized, "[PHONE_REDACTED]").to_string();

        // 3. Mask Credit Cards
        sanitized = CREDIT_CARD_REGEX.replace_all(&sanitized, "[CC_REDACTED]").to_string();

        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_email() {
        let text = "Contact me at user@example.com";
        let sanitized = Sanitizer::sanitize(text);
        assert_eq!(sanitized, "Contact me at [EMAIL_REDACTED]");
    }

    #[test]
    fn test_sanitize_phone() {
        let text = "Call 123-456-7890";
        let sanitized = Sanitizer::sanitize(text);
        assert_eq!(sanitized, "Call [PHONE_REDACTED]");
    }
}
