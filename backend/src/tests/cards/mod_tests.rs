use super::*;

#[test]
fn shares_a_character() {
    // 學 appears in both.
    assert!(shares_hanja_character("學校", "學生"));
}

#[test]
fn no_overlap_is_false() {
    assert!(!shares_hanja_character("學校", "生日"));
}

#[test]
fn hangul_only_strings_never_match() {
    // Also exercises the a_chars.is_empty() early return that an
    // all-empty-input case would hit identically, so there's no
    // separate empty-string test.
    assert!(!shares_hanja_character("학교", "학생"));
}

// No CJK-compatibility-ideograph test: this function does no Unicode
// normalization, so such a test could only pin down the absence of a
// feature, not catch a regression in one - same reasoning as the
// dropped NFD/NFC test in cards::check.
