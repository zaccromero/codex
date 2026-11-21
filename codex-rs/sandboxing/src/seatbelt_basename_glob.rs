//! Global basename rules stay effective after an ancestor directory moves.

pub(super) fn is_global_basename_rule(pattern: &str) -> bool {
    let Some(name) = pattern.strip_prefix("/**/") else {
        return false;
    };
    // Support a literal basename or one trailing wildcard, such as .env.*.
    // Other glob constructs retain conservative ancestor protection.
    let literal = name.strip_suffix('*').unwrap_or(name);
    !matches!(literal, "" | "." | "..")
        && !literal.contains(['/', '*', '?', '[', ']', '{', '}', '\\'])
}

#[cfg(test)]
mod tests {
    use super::is_global_basename_rule;

    #[test]
    fn accepts_relocation_invariant_literal_and_prefix_rules() {
        for pattern in [
            "/**/.env",
            "/**/.env.*",
            "/**/credentials",
            "/**/secret+file.txt",
        ] {
            assert!(is_global_basename_rule(pattern), "{pattern}");
        }
    }

    #[test]
    fn scoped_or_complex_rules_keep_ancestor_protection() {
        for pattern in [
            "/workspace/**/.env",
            "/workspace/**/.env.*",
            "/**/private/.env",
            "/**/",
            "/**/*",
            "/**/.*",
            "/**/..*",
            "/**/foo**",
            "/**/foo?*",
            "/**/[a-z]*",
            "/**/{a,b}*",
            "/**/foo\\*",
            "/**/.env.*/child",
        ] {
            assert!(!is_global_basename_rule(pattern), "{pattern}");
        }
    }
}
