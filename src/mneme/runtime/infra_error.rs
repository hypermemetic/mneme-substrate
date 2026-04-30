//! MNEME-37 — detect known infrastructure error classes in chat
//! response buffers before they get passed downstream as "no fenced
//! JSON block" parse failures.
//!
//! Today's failure mode (2026-04-30 probe): an expired OAuth token
//! caused the Claude Code SDK to pass `Failed to authenticate. API
//! Error: 401 ...` through the content stream. The substrate buffered
//! it as if it were normal model output, then `parse_step` on it
//! produced `NoStepBlock`. The user got `AllTrialsFailed` with no
//! signal that auth was the actual cause.
//!
//! This module is the line of defense: scan the buffered content
//! before treating it as model output, and return a typed error if
//! a known infra-error fingerprint matches. Pure function; no I/O.
//!
//! Pattern matching is conservative — for example, a real assistant
//! reply that mentions the substring `401` would NOT trigger the auth
//! detector unless it ALSO contains an authentication keyword. The
//! matchers are case-insensitive.

/// Classes of infrastructure error we know how to detect from chat
/// response content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfraErrorClass {
    /// 401 / OAuth expired / API key invalid.
    Auth,
    /// 429 / Anthropic rate limit hit.
    RateLimit,
    /// 5xx / service unavailable / overloaded.
    Service,
    /// Stream closed with no content at all.
    Empty,
}

impl InfraErrorClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auth => "AuthExpired",
            Self::RateLimit => "RateLimited",
            Self::Service => "ServiceError",
            Self::Empty => "EmptyResponse",
        }
    }
}

/// Scan a chat response buffer for known infrastructure-error
/// fingerprints. Returns `Some((class, message))` on a match or
/// `None` if nothing matches (presumably real model output, even if
/// it later fails to parse).
///
/// `message` is a short extract of the buffer suitable for logging.
pub fn detect_infra_error(buffer: &str) -> Option<(InfraErrorClass, String)> {
    if buffer.is_empty() {
        return Some((InfraErrorClass::Empty, "(empty buffer)".to_string()));
    }

    // Case-insensitive scan over the first ~2000 chars (errors land
    // at the start; if the model produced 10K of real output and
    // happened to mention "401" in a sentence that's not infra
    // error). The 2000-char window keeps the matcher fast.
    let head = if buffer.len() > 2000 {
        &buffer[..2000]
    } else {
        buffer
    };
    let lower = head.to_lowercase();

    // ---- Auth ----
    let has_auth_kw = lower.contains("authentication")
        || lower.contains("auth_error")
        || lower.contains("invalid api key")
        || lower.contains("oauth")
        || lower.contains("credentials");
    let has_401 = lower.contains("401");
    let has_failed_auth = lower.contains("failed to authenticate");
    if has_failed_auth
        || lower.contains("authentication_error")
        || lower.contains("invalid authentication credentials")
        || (has_401 && has_auth_kw)
    {
        return Some((
            InfraErrorClass::Auth,
            short_excerpt(head),
        ));
    }

    // ---- Rate limit ----
    let has_rate_kw = lower.contains("rate")
        || lower.contains("quota")
        || lower.contains("throttl");
    let has_429 = lower.contains("429");
    if lower.contains("rate_limit_error")
        || lower.contains("rate_limit_exceeded")
        || lower.contains("rate-limit")
        || (has_429 && has_rate_kw)
    {
        return Some((
            InfraErrorClass::RateLimit,
            short_excerpt(head),
        ));
    }

    // ---- Service errors (5xx) ----
    if lower.contains("service_unavailable")
        || lower.contains("overloaded_error")
        || lower.contains("internal_server_error")
        || lower.contains("500 internal")
        || lower.contains("502 bad gateway")
        || lower.contains("503 service")
        || lower.contains("504 gateway")
    {
        return Some((
            InfraErrorClass::Service,
            short_excerpt(head),
        ));
    }

    None
}

fn short_excerpt(s: &str) -> String {
    // First line, capped at 240 chars. Errors are typically
    // single-line JSON-ish blobs but defensive against the rare
    // multi-line case.
    let first_line = s.lines().next().unwrap_or("").trim();
    if first_line.len() > 240 {
        format!("{}…", &first_line[..240])
    } else {
        first_line.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real auth error from the 2026-04-30 probe.
    const REAL_AUTH_ERROR: &str = "Failed to authenticate. API Error: 401 \
        {\"type\":\"error\",\"error\":{\"type\":\"authentication_error\",\
        \"message\":\"Invalid authentication credentials\"},\"request_id\":\"req_xyz\"}";

    #[test]
    fn detects_real_auth_error_from_probe() {
        let result = detect_infra_error(REAL_AUTH_ERROR);
        assert!(result.is_some(), "should detect");
        assert_eq!(result.as_ref().unwrap().0, InfraErrorClass::Auth);
    }

    #[test]
    fn detects_authentication_error_keyword() {
        let s = "{\"error\":{\"type\":\"authentication_error\",\"message\":\"...\"}}";
        let r = detect_infra_error(s);
        assert_eq!(r.unwrap().0, InfraErrorClass::Auth);
    }

    #[test]
    fn detects_invalid_credentials_phrase() {
        let s = "API responded: Invalid authentication credentials";
        let r = detect_infra_error(s);
        assert_eq!(r.unwrap().0, InfraErrorClass::Auth);
    }

    #[test]
    fn case_insensitive_auth_match() {
        assert_eq!(
            detect_infra_error("FAILED TO AUTHENTICATE").unwrap().0,
            InfraErrorClass::Auth
        );
    }

    #[test]
    fn does_not_match_normal_assistant_text_mentioning_401() {
        // Apartment 401 example from the design notes — should NOT match
        // because no auth keyword is present.
        let s = "The recent property sold was apartment 401 in the Brooklyn \
                 district for $1.2M according to multiple sources.";
        assert!(detect_infra_error(s).is_none());
    }

    #[test]
    fn does_not_match_text_about_rate_or_quota_in_passing() {
        // "rate" appears in a normal sentence; no 429 or rate_limit
        // structural keyword.
        let s = "The exchange rate moved 2% on news of the central bank meeting. \
                 Quota systems were not affected.";
        assert!(detect_infra_error(s).is_none());
    }

    #[test]
    fn detects_rate_limit_error_keyword() {
        let s = "{\"error\":{\"type\":\"rate_limit_error\",\"message\":\"too many requests\"}}";
        assert_eq!(detect_infra_error(s).unwrap().0, InfraErrorClass::RateLimit);
    }

    #[test]
    fn detects_429_with_rate_keyword() {
        let s = "API Error: 429 — rate limit exceeded for organization-rpm";
        assert_eq!(detect_infra_error(s).unwrap().0, InfraErrorClass::RateLimit);
    }

    #[test]
    fn detects_overloaded_error() {
        let s = "{\"error\":{\"type\":\"overloaded_error\",\"message\":\"servers busy\"}}";
        assert_eq!(detect_infra_error(s).unwrap().0, InfraErrorClass::Service);
    }

    #[test]
    fn detects_503_service_pattern() {
        let s = "503 Service Unavailable — please retry";
        assert_eq!(detect_infra_error(s).unwrap().0, InfraErrorClass::Service);
    }

    #[test]
    fn detects_empty_buffer() {
        assert_eq!(detect_infra_error("").unwrap().0, InfraErrorClass::Empty);
    }

    #[test]
    fn does_not_match_normal_forecast_output() {
        let s = "Reasoning here.\n\n```json\n{\"action\": {\"type\": \"submit\", \
                 \"probability\": 0.42}, \"belief\": {...}}\n```\nDone.";
        assert!(detect_infra_error(s).is_none());
    }

    #[test]
    fn message_excerpt_truncates_long_first_line() {
        let s = format!("Failed to authenticate. {}", "x".repeat(500));
        let r = detect_infra_error(&s).unwrap();
        assert_eq!(r.0, InfraErrorClass::Auth);
        // 240 ASCII chars + '…' (1 char, 3 UTF-8 bytes) = 241 chars total.
        assert!(
            r.1.chars().count() <= 241,
            "excerpt should be ≤ 240 chars + 1-char ellipsis"
        );
        assert!(r.1.ends_with('…'));
    }

    #[test]
    fn class_as_str_stable_for_program_kind_field() {
        // These strings are written to error.json's `kind` field;
        // changing them is a wire-format break for downstream scripts.
        assert_eq!(InfraErrorClass::Auth.as_str(), "AuthExpired");
        assert_eq!(InfraErrorClass::RateLimit.as_str(), "RateLimited");
        assert_eq!(InfraErrorClass::Service.as_str(), "ServiceError");
        assert_eq!(InfraErrorClass::Empty.as_str(), "EmptyResponse");
    }
}
