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

#[test]
fn hints_exclude_the_card_itself_and_repeat_entries_in_studied_order() {
    let studied = |card_id: i64, hanja: &str, word: &str| StudiedHanja {
        card_id,
        hint: HanjaHint {
            hanja: hanja.into(),
            word: word.into(),
            trans_word: Some(format!("{word} gloss")),
        },
    };
    let studied = vec![
        studied(1, "學校", "학교"),
        studied(2, "學生", "학생"),
        studied(3, "生日", "생일"),
        studied(4, "學生", "학생"), // same hint as card 2
        studied(5, "先生", "선생"),
    ];
    let hints = hanja_hints_from(&studied, 1, &Some("學校".into()));
    assert_eq!(
        hints.iter().map(|h| h.hanja.as_str()).collect::<Vec<_>>(),
        ["學生"]
    );
    let hints = hanja_hints_from(&studied, 99, &Some("生".into()));
    assert_eq!(
        hints.iter().map(|h| h.hanja.as_str()).collect::<Vec<_>>(),
        ["學生", "生日", "先生"]
    );
    assert!(hanja_hints_from(&studied, 99, &None).is_empty());
    assert!(hanja_hints_from(&studied, 99, &Some(String::new())).is_empty());
    assert!(hanja_hints_from(&studied, 99, &Some("학교".into())).is_empty());
}
