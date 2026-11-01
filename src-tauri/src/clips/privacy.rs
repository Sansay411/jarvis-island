//! What the clipboard history must not keep.
//!
//! Password managers mark their copies with the Windows clipboard-history
//! formats (handled in `platform::clipboard`); this module is the second
//! line: copies from programs the user listed, and text that looks like a
//! credential. A credential that slips through is a harm; a note that was
//! not recorded is only an inconvenience, so the rules lean towards skipping.

/// Parts of program file names whose copies are never recorded. They are
/// matched as case-insensitive substrings of the process name, so
/// `KeePassXC.exe` is covered by `keepass`. The user edits the list in
/// Settings.
pub const DEFAULT_IGNORED_APPS: &[&str] = &["keepass", "1password", "bitwarden", "lastpass", "dashlane", "enpass", "nordpass", "protonpass", "roboform"];

/// Whether the program that copied is on the ignore list. Blank entries are
/// ignored (an empty substring would match every program).
pub fn app_ignored(process: &str, ignored: &[String]) -> bool {
    let process = process.to_lowercase();
    ignored.iter().map(|part| part.trim().to_lowercase()).filter(|part| !part.is_empty()).any(|part| process.contains(&part))
}

/// Whether the text looks like a credential: a private key, a known token
/// format, a JWT, a payment card number, or a long random single token.
pub fn looks_secret(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    if t.contains("-----BEGIN") && t.contains("PRIVATE KEY-----") {
        return true;
    }
    if is_card_number(t) {
        return true;
    }
    if t.chars().any(char::is_whitespace) {
        return false;
    }
    has_token_prefix(t) || is_jwt(t) || is_random_token(t)
}

/// Formats whose first characters give them away. Lengths keep a word that
/// merely starts the same (`sk-learn`) from matching.
fn has_token_prefix(t: &str) -> bool {
    let n = t.chars().count();
    let starts = |p: &str| t.starts_with(p);
    (starts("sk-") && n >= 20)
        || (["ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_"].iter().any(|p| starts(p)) && n >= 30)
        || ((starts("AKIA") || starts("ASIA")) && n == 20 && t.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
        || (starts("xox") && t.chars().nth(3).is_some_and(|c| "baprs".contains(c)) && t.chars().nth(4) == Some('-') && n >= 20)
        || (starts("AIza") && n == 39)
        || (starts("glpat-") && n >= 20)
}

fn is_jwt(t: &str) -> bool {
    let parts: Vec<&str> = t.split('.').collect();
    parts.len() == 3
        && t.starts_with("eyJ")
        && t.len() >= 30
        && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
}

/// 13–19 digits, optionally grouped by spaces or dashes, passing Luhn.
fn is_card_number(t: &str) -> bool {
    if !t.chars().all(|c| c.is_ascii_digit() || c == ' ' || c == '-') {
        return false;
    }
    let digits: Vec<u32> = t.chars().filter_map(|c| c.to_digit(10)).collect();
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
}

/// A long single token of base64-like characters with capitals, lowercase
/// and digits and no pattern to it. Hashes (one case) and identifiers
/// (no digits) stay out.
fn is_random_token(t: &str) -> bool {
    let n = t.chars().count();
    if !(24..=256).contains(&n) || !t.chars().all(|c| c.is_ascii_alphanumeric() || "_-+/=".contains(c)) {
        return false;
    }
    let has = |f: fn(&char) -> bool| t.chars().any(|c| f(&c));
    has(char::is_ascii_uppercase) && has(char::is_ascii_lowercase) && has(char::is_ascii_digit) && entropy_bits(t) >= 4.0
}

/// Shannon entropy in bits per byte.
fn entropy_bits(t: &str) -> f64 {
    let mut counts = [0u32; 256];
    for b in t.bytes() {
        counts[b as usize] += 1;
    }
    let n = t.len() as f64;
    counts.iter().filter(|&&c| c > 0).map(|&c| { let p = f64::from(c) / n; -p * p.log2() }).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_key_blocks_are_secret_but_certificates_are_not() {
        assert!(looks_secret("-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA\n-----END RSA PRIVATE KEY-----"));
        assert!(looks_secret("-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXk="));
        assert!(!looks_secret("-----BEGIN CERTIFICATE-----\nMIIDdzCCAl+gAwIBAgIE\n-----END CERTIFICATE-----"));
    }

    #[test]
    fn known_token_formats_are_secret() {
        assert!(looks_secret("sk-proj-abcdefghijklmnopqrstuvwxyz0123456789"));
        assert!(looks_secret("ghp_abcdefghijklmnopqrstuvwxyz0123456789"));
        assert!(looks_secret("github_pat_11ABCDEFG0abcdefghijkl_mnopqrstuvwxyz0123456789"));
        assert!(looks_secret("AKIAIOSFODNN7EXAMPLE"));
        assert!(looks_secret(&format!("xox{}-123456789012-1234567890123-abcdefghijklmnopqrstuvwx", "b")));
        assert!(looks_secret("AIzaSyA-abcdefghijklmnopqrstuvwxyz01234"));
        assert!(looks_secret("  glpat-abcdefghij0123456789  "));
    }

    #[test]
    fn a_jwt_is_secret() {
        assert!(looks_secret("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U"));
    }

    #[test]
    fn card_numbers_that_pass_the_luhn_check_are_secret() {
        assert!(looks_secret("4111 1111 1111 1111"));
        assert!(looks_secret("4111-1111-1111-1111"));
        assert!(looks_secret("5500005555555559"));
        assert!(!looks_secret("4111 1111 1111 1112"));
        assert!(!looks_secret("1234567890123"));
    }

    #[test]
    fn long_random_tokens_are_secret() {
        assert!(looks_secret("Zx9Qm4Lp7Wk2Rt8Vb3Nc6Hy1Jd5Fs0Ga"));
        assert!(looks_secret("Zx9Qm4Lp7Wk2Rt8Vb3Nc6Hy1Jd5Fs0Ga+/Qw3Er5Ty7Ui9Op1As=="));
    }

    #[test]
    fn ordinary_copies_are_not_secret() {
        for text in [
            "Встреча в четверг в 15:30, кабинет 204",
            "https://example.com/very/long/path?query=value&other=1234567890",
            "C:\\Users\\Павел\\Documents\\Отчёт_финальный_v2.docx",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "3F2504E0-4F89-11D3-9A0C-0305E82C3301",
            "fn main() { println!(\"hello\"); }",
            "getUserProfileDataByIdAndName",
            "Электроэнцефалографический",
            "+7 912 345 67 89",
            "pavel@example.com",
            "hunter2",
            "",
            "   ",
        ] {
            assert!(!looks_secret(text), "{text:?} was taken for a secret");
        }
    }

    #[test]
    fn programs_are_matched_by_a_part_of_their_name_ignoring_case() {
        let list = vec!["keepass".to_string(), "1Password".to_string()];
        assert!(app_ignored("KeePassXC.exe", &list));
        assert!(app_ignored("1password.exe", &list));
        assert!(!app_ignored("chrome.exe", &list));
    }

    #[test]
    fn blank_entries_do_not_match_everything() {
        assert!(!app_ignored("chrome.exe", &["".to_string(), "  ".to_string()]));
    }

    #[test]
    fn the_default_list_covers_common_password_managers() {
        let defaults: Vec<String> = DEFAULT_IGNORED_APPS.iter().map(|s| s.to_string()).collect();
        for exe in ["KeePass.exe", "Bitwarden.exe", "1Password.exe", "LastPass.exe", "Dashlane.exe", "Enpass.exe"] {
            assert!(app_ignored(exe, &defaults), "{exe} is not ignored by default");
        }
        assert!(!app_ignored("Code.exe", &defaults));
    }
}
