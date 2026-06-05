/// Produce a compact, display-friendly masked form of a secret.
///
/// The output keeps the leading 3 and trailing 4 characters so an operator can
/// recognize the key at a glance, while the middle is collapsed to a fixed
/// ellipsis. The middle never reveals how long the original was, because doing
/// so leaks bits of entropy.
///
/// IMPORTANT: the prefix+suffix preview reveals 7 characters, so for anything
/// short enough that those 7 would expose most/all of the secret we fall back
/// to a fully-starred string. We never return the raw value — even short
/// user-supplied keys must not round-trip in plaintext through the masked
/// list/reveal endpoints.
///
/// Examples:
///   ""                    -> ""
///   "abc"                 -> "***"
///   "1234567"             -> "*******"
///   "12345678"            -> "********"
///   "sk-1234567890abcdef" -> "sk-…cdef"
const MASK_PREVIEW_PREFIX: usize = 3;
const MASK_PREVIEW_SUFFIX: usize = 4;
/// Minimum hidden characters required before we show the prefix/suffix preview.
const MASK_MIN_HIDDEN: usize = 4;

pub fn mask_secret(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let len = chars.len();

    if len == 0 {
        return String::new();
    }

    // Only reveal the prefix/suffix preview when enough of the secret stays
    // hidden; otherwise fully star it. This guarantees short secrets are never
    // returned in (near-)plaintext.
    let preview_len = MASK_PREVIEW_PREFIX + MASK_PREVIEW_SUFFIX;
    if len < preview_len + MASK_MIN_HIDDEN {
        return "*".repeat(len);
    }

    let prefix: String = chars.iter().take(MASK_PREVIEW_PREFIX).collect();
    let suffix: String = chars.iter().skip(len - MASK_PREVIEW_SUFFIX).collect();
    format!("{prefix}…{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_secret() {
        assert_eq!(mask_secret(""), "");
        assert_eq!(mask_secret("abc"), "***");
        // Short secrets (< 11 chars) are fully starred — never plaintext.
        assert_eq!(mask_secret("1234567"), "*******");
        assert_eq!(mask_secret("12345678"), "********");
        assert_eq!(mask_secret("sk-abc12"), "********");
        assert_eq!(mask_secret("0123456789"), "**********");
        // 11+ chars: prefix(3)…suffix(4) preview, middle length hidden.
        assert_eq!(mask_secret("0123456789a"), "012…789a");
        assert_eq!(mask_secret("sk-1234567890abcdef"), "sk-…cdef");
        assert_eq!(
            mask_secret("sk-apex-0f45bc06fa770eb934e894f7b036ab0c"),
            "sk-…ab0c"
        );
    }
}
