//! Recognize basename denials that remain effective after ancestor moves.

pub(super) fn is_global_basename_rule(pattern: &str) -> bool {
    let Some(name) = pattern.strip_prefix("/**/") else {
        return false;
    };
    // One component, with a literal prefix and at most one single-star wildcard.
    // A single star compiles to [^/]*, so even a wildcard before a literal suffix
    // cannot change the rule's scope when an ancestor moves. Keep globstars,
    // escaping, classes, alternation and scoped paths on the conservative path.
    if name.contains(['/', '?', '[', ']', '{', '}', '\\']) {
        return false;
    }
    let (prefix, suffix) = name.split_once('*').unwrap_or((name, ""));
    !matches!(prefix, "" | "." | "..") && !suffix.contains('*')
}

#[cfg(test)]
mod tests {
    use super::is_global_basename_rule;

    #[test]
    fn accepts_global_literal_prefix_and_suffix_rules() {
        for pattern in [
            "/**/.env",
            "/**/credentials",
            "/**/secret+file.txt",
            "/**/.env.*",
            "/**/.env.*.local",
            "/**/secret-*.json",
        ] {
            assert!(is_global_basename_rule(pattern), "{pattern}");
        }
    }

    #[test]
    fn keeps_scoped_and_complex_rules_conservative() {
        for pattern in [
            "**/.env.*.local",
            "/workspace/**/.env.*.local",
            "/**/private/.env",
            "/**/",
            "/**/.",
            "/**/..",
            "/**/*",
            "/**/.*",
            "/**/..*",
            "/**/*.env",
            "/**/.env.**.local",
            "/**/.env.*.*.local",
            "/**/.env.?.local",
            "/**/[a-z]*",
            "/**/{a,b}*",
            "/**/foo\\*",
            "/**/.env.*/child",
        ] {
            assert!(!is_global_basename_rule(pattern), "{pattern}");
        }
    }
}
