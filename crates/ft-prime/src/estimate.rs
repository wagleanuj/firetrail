//! Token estimation.
//!
//! ADR-0019 acknowledges that an approximate token estimator within ~5% is
//! adequate for budget accounting. We use the standard `chars / 4`
//! approximation used informally by `OpenAI`'s tokenizer docs — close enough
//! for English-and-code text without pulling in a tokenizer dependency.

/// Approximate token count for `text`.
///
/// Uses `ceil(chars / 4)` so that any non-empty input yields at least one
/// token. Empty input yields zero.
#[must_use]
pub fn estimate_tokens(text: &str) -> usize {
    let chars = text.chars().count();
    if chars == 0 { 0 } else { chars.div_ceil(4) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_zero() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn short_is_at_least_one() {
        assert_eq!(estimate_tokens("a"), 1);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
    }

    #[test]
    fn monotonic_in_length() {
        let mut prev = 0;
        let mut buf = String::new();
        for _ in 0..200 {
            buf.push('x');
            let t = estimate_tokens(&buf);
            assert!(t >= prev, "non-monotonic: {prev} -> {t}");
            prev = t;
        }
    }
}
