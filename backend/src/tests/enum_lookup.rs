use super::*;

#[test]
fn every_registered_table_maps_to_its_labels_child() {
    assert_eq!(
        labels_table("grammar_patterns"),
        ("grammar_patterns_labels", "grammar_pattern_id")
    );
    assert_eq!(
        labels_table("origin_types"),
        ("origin_types_labels", "origin_type_id")
    );
    assert_eq!(labels_table("grades"), ("grades_labels", "grade_id"));
    assert_eq!(
        labels_table("speech_levels"),
        ("speech_levels_labels", "speech_level_id")
    );
    assert_eq!(labels_table("tenses"), ("tenses_labels", "tense_id"));
}

#[test]
fn parts_of_speech_is_the_irregular_case() {
    // Every other table's fk column is `<table_singular>_id`; this one
    // is `pos_id`, not `parts_of_speech_id` - the one mapping that
    // can't be derived mechanically from `table`.
    assert_eq!(
        labels_table("parts_of_speech"),
        ("parts_of_speech_labels", "pos_id")
    );
}

#[test]
#[should_panic(expected = "no _labels table registered")]
fn unregistered_table_panics() {
    labels_table("not_a_real_table");
}
