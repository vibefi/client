use std::path::PathBuf;

/// Read an env var and parse it as a boolean (1/true/yes/on → true).
/// Returns `None` if the variable is unset or empty.
pub fn parse_bool_env(key: &str) -> Option<bool> {
    let val = std::env::var(key).ok()?;
    let trimmed = val.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return None;
    }
    Some(matches!(trimmed.as_str(), "1" | "true" | "yes" | "on"))
}

/// Read an env var as a trimmed, non-empty string.
pub fn parse_string_env(key: &str) -> Option<String> {
    let val = std::env::var(key).ok()?;
    let trimmed = val.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Read an env var as a `PathBuf`.
#[allow(dead_code)]
pub fn parse_path_env(key: &str) -> Option<PathBuf> {
    parse_string_env(key).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Each test uses a unique env var name to avoid races when tests run in parallel.

    #[test]
    fn parse_bool_env_truthy_values() {
        for val in ["1", "true", "yes", "on", " TRUE ", " On "] {
            unsafe { std::env::set_var("_TEST_BOOL_TRUTHY", val) };
            assert_eq!(
                parse_bool_env("_TEST_BOOL_TRUTHY"),
                Some(true),
                "expected true for {val:?}"
            );
        }
        unsafe { std::env::remove_var("_TEST_BOOL_TRUTHY") };
    }

    #[test]
    fn parse_bool_env_falsy_values() {
        for val in ["0", "false", "no", "off", "anything"] {
            unsafe { std::env::set_var("_TEST_BOOL_FALSY", val) };
            assert_eq!(
                parse_bool_env("_TEST_BOOL_FALSY"),
                Some(false),
                "expected false for {val:?}"
            );
        }
        unsafe { std::env::remove_var("_TEST_BOOL_FALSY") };
    }

    #[test]
    fn parse_bool_env_unset() {
        unsafe { std::env::remove_var("_TEST_BOOL_UNSET") };
        assert_eq!(parse_bool_env("_TEST_BOOL_UNSET"), None);
    }

    #[test]
    fn parse_string_env_trims_whitespace() {
        unsafe { std::env::set_var("_TEST_STR_TRIM", "  hello  ") };
        assert_eq!(parse_string_env("_TEST_STR_TRIM"), Some("hello".to_string()));
        unsafe { std::env::remove_var("_TEST_STR_TRIM") };
    }

    #[test]
    fn parse_string_env_empty_is_none() {
        unsafe { std::env::set_var("_TEST_STR_EMPTY", "   ") };
        assert_eq!(parse_string_env("_TEST_STR_EMPTY"), None);
        unsafe { std::env::remove_var("_TEST_STR_EMPTY") };
    }

    #[test]
    fn parse_path_env_works() {
        unsafe { std::env::set_var("_TEST_PATH_ENV", "/tmp/test") };
        assert_eq!(
            parse_path_env("_TEST_PATH_ENV"),
            Some(PathBuf::from("/tmp/test"))
        );
        unsafe { std::env::remove_var("_TEST_PATH_ENV") };
    }
}
