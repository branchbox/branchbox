//! Shared helpers for environment-variable placeholder parsing.

pub(crate) fn is_valid_env_var_name(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    if first != '_' && !first.is_ascii_alphabetic() {
        return false;
    }

    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

pub(crate) fn parse_env_reference(value: &str) -> Option<&str> {
    let value = value.trim();
    if let Some(env_name) = value
        .strip_prefix("${")
        .and_then(|raw| raw.strip_suffix('}'))
        .filter(|candidate| is_valid_env_var_name(candidate))
    {
        return Some(env_name);
    }

    value
        .strip_prefix('$')
        .filter(|candidate| is_valid_env_var_name(candidate))
}

pub(crate) fn looks_like_env_placeholder(value: &str) -> bool {
    parse_env_reference(value).is_some()
}
