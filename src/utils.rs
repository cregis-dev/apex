pub fn mask_secret(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 7 {
        return "*".repeat(chars.len());
    }
    let prefix: String = chars.iter().take(3).collect();
    let suffix: String = chars.iter().skip(chars.len() - 4).collect();
    let masked = "*".repeat(chars.len() - 7);
    format!("{}{}{}", prefix, masked, suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_secret() {
        assert_eq!(mask_secret("1234567"), "*******");
        assert_eq!(mask_secret("12345678"), "123*5678");
        assert_eq!(mask_secret("sk-1234567890abcdef"), "sk-************cdef");
    }
}
