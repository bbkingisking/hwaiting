//! Fixed classification vocabulary shared by storage, API schemas, and catalogs.
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

macro_rules! classification {
    ($name:ident { $($variant:ident => $slug:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
        #[sqlx(type_name = "TEXT")]
        pub enum $name {
            $(#[serde(rename = $slug)] #[schema(rename = $slug)] #[sqlx(rename = $slug)] $variant),+
        }
        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $slug),+ }
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

classification!(PartOfSpeech {
    Verb => "verb",
    Noun => "noun",
    Adjective => "adjective",
    Adverb => "adverb",
    BoundNoun => "bound-noun",
    Pronoun => "pronoun",
    Numeral => "numeral",
    Interjection => "interjection",
    Determiner => "determiner",
    AuxiliaryAdjective => "auxiliary-adjective",
    AuxiliaryVerb => "auxiliary-verb",
    Particle => "particle",
    NoPos => "no-pos",
});

classification!(OriginType {
    NativeKorean => "native-korean",
    SinoKorean => "sino-korean",
    Loanword => "loanword",
    Hybrid => "hybrid",
});

classification!(Grade {
    Beginner => "beginner",
    Intermediate => "intermediate",
    Advanced => "advanced",
});

classification!(SpeechLevel {
    HaeChe => "hae-che",
    HaeyoChe => "haeyo-che",
    HasipsioChe => "hasipsio-che",
    HaeraChe => "haera-che",
    HaoChe => "hao-che",
    HageChe => "hage-che",
});

classification!(Tense {
    Present => "present",
    Past => "past",
    Future => "future",
});

#[derive(Serialize, ToSchema)]
pub struct ClassificationsResponse {
    pub parts_of_speech: Vec<PartOfSpeech>,
    pub origin_types: Vec<OriginType>,
    pub grades: Vec<Grade>,
    pub speech_levels: Vec<SpeechLevel>,
    pub tenses: Vec<Tense>,
}

#[utoipa::path(get, path = "/api/resources/classifications",
    description = "List the fixed classification slugs in presentation order. Display labels belong to clients.",
    responses((status = 200, description = "Classification catalogs", body = ClassificationsResponse)),
    security(("cookie_auth" = [])), tag = "resources")]
pub async fn list_classifications(
    _auth: crate::auth::AuthUser,
) -> axum::Json<ClassificationsResponse> {
    axum::Json(ClassificationsResponse {
        parts_of_speech: PartOfSpeech::ALL.to_vec(),
        origin_types: OriginType::ALL.to_vec(),
        grades: Grade::ALL.to_vec(),
        speech_levels: SpeechLevel::ALL.to_vec(),
        tenses: Tense::ALL.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn catalogs_match_database_and_openapi() {
        use utoipa::OpenApi;
        let pool = crate::test_support::test_pool().await;
        let catalog =
            serde_json::to_value(list_classifications(crate::auth::AuthUser(1)).await.0).unwrap();
        let spec = serde_json::to_value(crate::openapi::ApiDoc::openapi()).unwrap();
        for (field, table, schema) in [
            ("parts_of_speech", "parts_of_speech", "PartOfSpeech"),
            ("origin_types", "origin_types", "OriginType"),
            ("grades", "grades", "Grade"),
            ("speech_levels", "speech_levels", "SpeechLevel"),
            ("tenses", "tenses", "Tense"),
        ] {
            let stored: Vec<String> =
                sqlx::query_scalar(&format!("SELECT slug FROM {table} ORDER BY id"))
                    .fetch_all(&pool)
                    .await
                    .unwrap();
            assert_eq!(catalog[field], serde_json::json!(stored), "{field}");
            assert_eq!(
                catalog[field], spec["components"]["schemas"][schema]["enum"],
                "{schema}"
            );
        }
    }

    #[test]
    fn classification_edits_reject_unknown_slugs_and_preserve_patch_semantics() {
        for field in ["pos", "origin_type", "grade", "speech_level", "tense"] {
            assert!(
                serde_json::from_value::<crate::admin::UpdateCardRequest>(
                    serde_json::json!({field: "unknown"})
                )
                .is_err(),
                "{field}"
            );
        }
        let absent: crate::admin::UpdateCardRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(absent.pos, None);
        let cleared: crate::admin::UpdateCardRequest =
            serde_json::from_str(r#"{"pos":null}"#).unwrap();
        assert_eq!(cleared.pos, Some(None));
        let set: crate::admin::UpdateCardRequest =
            serde_json::from_str(r#"{"pos":"noun"}"#).unwrap();
        assert_eq!(set.pos, Some(Some(PartOfSpeech::Noun)));
    }
}
