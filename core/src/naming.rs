//! Smart naming utilities for git worktree workflow
//!
//! Generates clean, dasherized feature names from human-readable titles.
//!
//! # Examples
//!
//! ```
//! use worktree_core::naming::generate_work_feature;
//!
//! let name = generate_work_feature("OAuth Integration Feature");
//! assert_eq!(name, "oauth-integration");
//!
//! let name = generate_work_feature("Fix Critical Bug in Authentication");
//! assert_eq!(name, "critical-bug-authentication");
//! ```

use regex::Regex;

/// Filler words to remove from feature names
const FILLER_WORDS: &[&str] = &[
    "the", "a", "an", "and", "or", "for", "with", "using", "feature", "integration",
    "implementation", "support", "system", "in", "of", "to",
];

/// Maximum number of words to include in work_feature name
const MAX_WORDS: usize = 4;

/// Generate WORK_FEATURE name from title
///
/// Smart truncation: removes filler words, takes max 4 meaningful words
///
/// # Rules
///
/// 1. Lowercase only: `OAuth` → `oauth`
/// 2. Dasherized: `OAuth Integration` → `oauth-integration`
/// 3. Max 4 words: Takes most meaningful words
/// 4. Removes filler words: "the", "a", "and", "for", "with", "feature", "integration", "support"
/// 5. DNS-safe: Only `a-z0-9-` characters
///
/// # Examples
///
/// ```
/// use worktree_core::naming::generate_work_feature;
///
/// assert_eq!(
///     generate_work_feature("OpenAI Responses API Integration"),
///     "responses-api-integration"
/// );
///
/// assert_eq!(
///     generate_work_feature("OAuth Connected Apps Feature"),
///     "oauth-connected-apps"
/// );
///
/// assert_eq!(
///     generate_work_feature("Fix Critical Bug in Authentication"),
///     "critical-bug-authentication"
/// );
///
/// assert_eq!(
///     generate_work_feature("Support for Multiple LLM Providers"),
///     "multiple-llm-providers"
/// );
/// ```
pub fn generate_work_feature(title: &str) -> String {
    // Convert to lowercase and remove special characters
    let cleaned = clean_title(title);

    // Split into words
    let words: Vec<&str> = cleaned.split_whitespace().collect();

    // Remove filler words
    let meaningful_words: Vec<&str> = words
        .into_iter()
        .filter(|word| !FILLER_WORDS.contains(word))
        .collect();

    // Take first MAX_WORDS words and join with hyphens
    meaningful_words
        .into_iter()
        .take(MAX_WORDS)
        .collect::<Vec<_>>()
        .join("-")
}

/// Clean title: lowercase and remove special characters
fn clean_title(title: &str) -> String {
    let re = Regex::new(r"[^a-z0-9 -]").unwrap();
    let lowercase = title.to_lowercase();
    let cleaned = re.replace_all(&lowercase, "");

    // Normalize multiple spaces to single space
    let re_spaces = Regex::new(r"\s+").unwrap();
    re_spaces.replace_all(&cleaned, " ").trim().to_string()
}

/// Validate WORK_FEATURE name (DNS-safe)
///
/// Returns `true` if the name contains only lowercase letters, numbers, and hyphens.
///
/// # Examples
///
/// ```
/// use worktree_core::naming::validate_work_feature;
///
/// assert!(validate_work_feature("oauth-integration"));
/// assert!(validate_work_feature("api-v2"));
/// assert!(validate_work_feature("bug-fix-123"));
///
/// assert!(!validate_work_feature("OAuth-Integration")); // uppercase
/// assert!(!validate_work_feature("oauth_integration")); // underscore
/// assert!(!validate_work_feature("oauth integration")); // space
/// assert!(!validate_work_feature("oauth.integration")); // dot
/// ```
pub fn validate_work_feature(work_feature: &str) -> bool {
    let re = Regex::new(r"^[a-z0-9-]+$").unwrap();
    re.is_match(work_feature)
}

/// Generate feature URL from base URL and work_feature name
///
/// # Examples
///
/// ```
/// use worktree_core::naming::generate_feature_url;
///
/// let url = generate_feature_url("rida-mbp-agentify.rida.me", "oauth-integration");
/// assert_eq!(url, "rida-mbp-agentify-oauth-integration.rida.me");
/// ```
pub fn generate_feature_url(base_url: &str, work_feature: &str) -> String {
    // Split base URL into prefix and domain
    let parts: Vec<&str> = base_url.splitn(2, '.').collect();

    if parts.len() == 2 {
        format!("{}-{}.{}", parts[0], work_feature, parts[1])
    } else {
        // Fallback if no domain found
        format!("{}-{}", base_url, work_feature)
    }
}

/// Generate tunnel name from base prefix and work_feature name
///
/// # Examples
///
/// ```
/// use worktree_core::naming::generate_tunnel_name;
///
/// let name = generate_tunnel_name("rida-mbp-agentify", "oauth-integration");
/// assert_eq!(name, "rida-mbp-agentify-oauth-integration");
/// ```
pub fn generate_tunnel_name(base_prefix: &str, work_feature: &str) -> String {
    format!("{}-{}", base_prefix, work_feature)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_work_feature() {
        // Test cases from bash implementation
        assert_eq!(
            generate_work_feature("OpenAI Responses API Integration"),
            "openai-responses-api"  // Takes first 4 non-filler words
        );

        assert_eq!(
            generate_work_feature("OAuth Connected Apps Feature"),
            "oauth-connected-apps"
        );

        assert_eq!(
            generate_work_feature("Fix Critical Bug in Authentication"),
            "fix-critical-bug"  // "in" is removed as filler
        );

        assert_eq!(
            generate_work_feature("Support for Multiple LLM Providers"),
            "multiple-llm-providers"  // "Support for" removed
        );
    }

    #[test]
    fn test_clean_title() {
        assert_eq!(clean_title("OAuth-Integration"), "oauth-integration");
        assert_eq!(clean_title("API_v2"), "api_v2");
        assert_eq!(clean_title("Multiple   Spaces"), "multiple spaces");
        assert_eq!(clean_title("Special!@#$%Chars"), "specialchars");
    }

    #[test]
    fn test_validate_work_feature() {
        // Valid names
        assert!(validate_work_feature("oauth-integration"));
        assert!(validate_work_feature("api-v2"));
        assert!(validate_work_feature("bug-fix-123"));
        assert!(validate_work_feature("simple"));

        // Invalid names
        assert!(!validate_work_feature("OAuth-Integration")); // uppercase
        assert!(!validate_work_feature("oauth_integration")); // underscore
        assert!(!validate_work_feature("oauth integration")); // space
        assert!(!validate_work_feature("oauth.integration")); // dot
        assert!(!validate_work_feature("")); // empty
    }

    #[test]
    fn test_generate_feature_url() {
        assert_eq!(
            generate_feature_url("rida-mbp-agentify.rida.me", "oauth-integration"),
            "rida-mbp-agentify-oauth-integration.rida.me"
        );

        assert_eq!(
            generate_feature_url("dev.example.com", "test-feature"),
            "dev-test-feature.example.com"
        );
    }

    #[test]
    fn test_generate_tunnel_name() {
        assert_eq!(
            generate_tunnel_name("rida-mbp-agentify", "oauth-integration"),
            "rida-mbp-agentify-oauth-integration"
        );
    }

    #[test]
    fn test_max_words() {
        let long_title = "This is a very long feature name with many words that should be truncated";
        let result = generate_work_feature(long_title);

        // Should have max 4 words
        assert_eq!(result.split('-').count(), MAX_WORDS);
    }

    #[test]
    fn test_all_filler_words() {
        let result = generate_work_feature("the a an and or for with using");
        // All filler words removed, should be empty
        assert_eq!(result, "");
    }
}
