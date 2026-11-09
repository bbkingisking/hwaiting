use super::*;

fn parse_fields(raw: &str) -> Result<Vec<FieldName>, serde::de::value::Error> {
    let deserializer: StrDeserializer<'_, serde::de::value::Error> = raw.into_deserializer();
    deserialize_field_list(deserializer)
}

#[test]
fn single_field() {
    assert_eq!(parse_fields("pos").unwrap(), vec![FieldName::Pos]);
}

#[test]
fn comma_list() {
    assert_eq!(
        parse_fields("pos,grade").unwrap(),
        vec![FieldName::Pos, FieldName::Grade]
    );
}

#[test]
fn multi_word_snake_case_field() {
    assert_eq!(
        parse_fields("grammar_pattern").unwrap(),
        vec![FieldName::GrammarPattern]
    );
}

#[test]
fn tolerates_surrounding_whitespace() {
    assert_eq!(
        parse_fields(" pos , grade ").unwrap(),
        vec![FieldName::Pos, FieldName::Grade]
    );
}

#[test]
fn empty_string_is_empty_list() {
    assert_eq!(parse_fields("").unwrap(), Vec::<FieldName>::new());
}

#[test]
fn unknown_field_name_is_rejected() {
    assert!(parse_fields("not_a_real_field").is_err());
}

#[test]
fn wire_values_are_snake_case_not_the_rust_variant_name() {
    // The enum variant is `GrammarPattern`; only the snake_case wire
    // form is accepted.
    assert!(parse_fields("GrammarPattern").is_err());
}
