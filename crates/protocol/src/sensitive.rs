//! The canonical sensitive-path floor: substrings that mark a path as holding secrets. It
//! lives in `protocol` — the dependency-free leaf crate — so every enforcer shares one list
//! without pulling in engine logic. Keeping one list here makes drift between enforcers
//! impossible.

/// Lowercase substrings that mark a path as sensitive. Matching is case-insensitive (see
/// [`is_sensitive`]). Symlink-to-secret escapes are out of scope for this string floor.
pub const SENSITIVE_MARKERS: &[&str] = &[
    ".env", ".ssh/", ".ssh", ".git/", ".git", "id_rsa", ".aws/", ".aws",
];

/// True if `s` names a sensitive path under the floor. Case-insensitive (ASCII) substring
/// match, so `.ENV` / `.AWS/...` cannot slip past on case-insensitive filesystems.
pub fn is_sensitive(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    SENSITIVE_MARKERS.iter().any(|m| lower.contains(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_secrets_case_insensitively() {
        assert!(is_sensitive(".env"));
        assert!(is_sensitive("config/.ENV.local"));
        assert!(is_sensitive(".ssh/id_rsa"));
        assert!(is_sensitive("ID_RSA"));
        assert!(is_sensitive("config/production.env"));
        assert!(!is_sensitive("src/main.rs"));
        assert!(!is_sensitive("crates/engine/src/gate.rs"));
    }
}
