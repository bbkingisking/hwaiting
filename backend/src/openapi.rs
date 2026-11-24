use utoipa::{
    Modify, OpenApi,
    openapi::security::{ApiKey, ApiKeyValue, SecurityRequirement, SecurityScheme},
    openapi::{
        Ref, Required,
        content::ContentBuilder,
        path::{ParameterBuilder, ParameterIn},
        response::ResponseBuilder,
        schema::{ObjectBuilder, Type},
    },
};

fn error_response(description: &str) -> utoipa::openapi::RefOr<utoipa::openapi::Response> {
    ResponseBuilder::new()
        .description(description)
        .content(
            "application/json",
            ContentBuilder::new()
                .schema(Some(Ref::from_schema_name("ErrorResponse")))
                .build(),
        )
        .build()
        .into()
}

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        // No repository license is declared. Avoid an empty license placeholder.
        if openapi
            .info
            .license
            .as_ref()
            .is_some_and(|license| license.name.is_empty())
        {
            openapi.info.license = None;
        }
        openapi.servers = Some(vec![utoipa::openapi::Server::new("/")]);
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "cookie_auth",
                SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new(
                    crate::auth_cookie::COOKIE_NAME,
                ))),
            );
        }
        // Shared failures follow the same authentication and middleware rules as
        // the router. Preserve endpoint-specific descriptions when present.
        for path in openapi.paths.paths.values_mut() {
            for operation in [
                &mut path.get,
                &mut path.post,
                &mut path.put,
                &mut path.patch,
                &mut path.delete,
            ]
            .into_iter()
            .flatten()
            {
                operation
                    .responses
                    .responses
                    .entry("500".into())
                    .or_insert_with(|| error_response("Internal server error"));
                if operation.security.as_ref().is_some_and(|requirements| {
                    requirements.iter().any(|requirement| {
                        *requirement
                            == SecurityRequirement::new("cookie_auth", std::iter::empty::<&str>())
                    })
                }) {
                    operation
                        .responses
                        .responses
                        .entry("401".into())
                        .or_insert_with(|| {
                            error_response("Missing or invalid authentication cookie")
                        });
                }
            }
        }
        // Keep the write requirement executable in the API reference and visible
        // to generated clients, including unauthenticated login endpoints.
        for path in openapi.paths.paths.values_mut() {
            for operation in [
                &mut path.post,
                &mut path.put,
                &mut path.patch,
                &mut path.delete,
            ]
            .into_iter()
            .flatten()
            {
                operation
                    .responses
                    .responses
                    .entry("403".into())
                    .and_modify(|response| {
                        if let utoipa::openapi::RefOr::T(response) = response {
                            response.description.push_str(
                                "; also returned for a missing or invalid X-Requested-With header",
                            );
                        }
                    })
                    .or_insert_with(|| {
                        error_response("Missing or invalid X-Requested-With header")
                    });
                // These routes publish SSE notifications through the mutation adapters.
                if matches!(
                    operation.operation_id.as_deref(),
                    Some(
                        "check_answer"
                            | "suppress_card"
                            | "unsuppress_card"
                            | "update_settings"
                            | "import_data"
                            | "save_admin_card"
                            | "optimize_fsrs"
                            | "reset_fsrs_parameters"
                    )
                ) {
                    operation.parameters.get_or_insert_with(Vec::new).push(
                    ParameterBuilder::new()
                        .name("X-Mutation-Id")
                        .parameter_in(ParameterIn::Header)
                        .required(Required::False)
                        .description(Some("Optional UUID echoed in SSE notifications for successful mutations, including import-triggered resync-required; invalid UUIDs are ignored. Connection and lag notifications omit it. Not an identity or idempotency key."))
                        .schema(Some(ObjectBuilder::new().schema_type(Type::String)))
                        .build(),
                );
                }
                operation.parameters.get_or_insert_with(Vec::new).push(
                    ParameterBuilder::new()
                        .name("X-Requested-With")
                        .parameter_in(ParameterIn::Header)
                        .required(Required::True)
                        .description(Some("CSRF protection: cross-origin browser writes require an allowed CORS origin."))
                        .schema(Some(ObjectBuilder::new().schema_type(Type::String)
                            .default(Some(serde_json::json!(crate::auth_cookie::CSRF_VALUE)))
                            .enum_values(Some([crate::auth_cookie::CSRF_VALUE]))))
                        .build(),
                );
            }
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "hwaiting API",
        version = "1.0.0",
        description = "Korean FSRS flashcard backend. Authentication uses an HttpOnly JWT cookie. All POST, PUT, PATCH, and DELETE requests require X-Requested-With: hwaiting; cross-origin browser clients must be explicitly allowed through CORS."
    ),
    paths(
        crate::events::subscribe,
        crate::passkey::capabilities,
        crate::auth::login,
        crate::auth::signup,
        crate::auth_cookie::logout,
        crate::passkey::register_start,
        crate::passkey::register_finish,
        crate::passkey::login_start,
        crate::passkey::login_finish,
        crate::passkey::list_passkeys,
        crate::passkey::add_passkey_start,
        crate::passkey::add_passkey_finish,
        crate::passkey::delete_passkey,
        crate::passkey::update_passkey,
        crate::cards::get_review_queue,
        crate::translation_language::available_translation_languages,
        crate::cards::list_grammar_patterns,
        crate::cards::check_answer,
        crate::cards::validate_card_attempt,
        crate::feedback::submit_feedback,
        crate::feedback::list_feedback,
        crate::feedback::delete_feedback,
        crate::cards::suppress_card,
        crate::cards::unsuppress_card,
        crate::cards::list_suppressed_cards,
        crate::cards::get_hanja_drill,
        crate::cards::get_today_stats,
        crate::cards::get_history,
        crate::cards::optimize_fsrs,
        crate::cards::reset_fsrs_parameters,
        crate::user::get_profile,
        crate::user::get_settings,
        crate::user::update_settings,
        crate::export_import::export_data,
        crate::export_import::import_data,
        crate::admin::list_users,
        crate::admin::search_card_summaries,
        crate::admin::get_admin_card,
        crate::admin::get_card_inflections,
        crate::admin::save_admin_card,
        crate::health_check,
    ),
    components(schemas(
        crate::events::Notification,
        crate::error::ErrorResponse,
        crate::auth::LoginRequest,
        crate::auth::SignupRequest,
        crate::auth::AuthResponse,
        crate::passkey::PasskeySummary,
        crate::passkey::UpdatePasskeyRequest,
        crate::passkey::ListPasskeysResponse,
        crate::passkey::Capabilities,
        crate::cards::ReviewQueueResponse,
        crate::cards::ValidateRequest,
        crate::cards::ValidateResponse,
        crate::cards::CheckRequest,
        crate::cards::CheckResponse,
        crate::feedback::FeedbackRequest,
        crate::feedback::FeedbackResponse,
        crate::feedback::ListFeedbackResponse,
        crate::cards::TodayStatsResponse,
        crate::cards::SuppressedCardsResponse,
        crate::cards::HanjaDrill,
        crate::cards::HanjaDrillResponse,
        crate::cards::HistoryResponse,
        crate::cards::HistorySummary,
        crate::cards::OptimizeFsrsResponse,
        crate::cards::HistoryBreakdownResponse,
        crate::user::UserProfile,
        crate::user::UserSettings,
        crate::user::UpdateSettingsRequest,
        crate::export_import::ExportData,
        crate::export_import::WrongAnswerExport,
        crate::export_import::ImportDataRequest,
        crate::export_import::ImportDataResponse,
        crate::admin::AdminUserSummary,
        crate::admin::ListUsersResponse,
        crate::admin::CardInflectionsResponse,
        crate::admin::UpdateCardRequest,
    )),
    modifiers(&SecurityAddon),
    tags(
        (name = "auth", description = "Deployment-controlled sign-up and login, via either username/password or passkey (WebAuthn) - two independent ways to reach the same JWT"),
        (name = "reviews", description = "Review queue, card freshness, and answer submission"),
        (name = "user", description = "Profile, settings, data export/import"),
        (name = "admin", description = "Requires is_admin = true on the JWT's user"),
        (name = "cards", description = "Card languages and suppression"),
        (name = "scheduling", description = "Personal FSRS parameter fitting, reset, and card rescheduling"),
        (name = "stats", description = "Study activity statistics"),
        (name = "feedback", description = "Sentence content feedback for admin triage"),
        (name = "drills", description = "Practice activities outside scheduled reviews"),
        (name = "resources", description = "Shared reference resources for authenticated users"),
        (name = "misc", description = "Health check and deployment capabilities"),
    )
)]
// Convention for every list-shaped response registered above (ListUsersResponse,
// SearchCardsResponse, SuppressedCardsResponse, ...): the collection lives in a
// field named after the resource, plural - `users`, `codes`, `cards` - never a
// generic `items`/`data` key. Keep new list endpoints consistent with that
// rather than introducing another wrapper name.
pub struct ApiDoc;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_failures_match_authentication_and_write_middleware() {
        let spec = serde_json::to_value(ApiDoc::openapi()).unwrap();
        for item in spec["paths"].as_object().unwrap().values() {
            for (method, operation) in item.as_object().unwrap() {
                assert!(operation["responses"].get("500").is_some());
                if operation.get("security").is_some() {
                    assert!(operation["responses"].get("401").is_some());
                }
                if matches!(method.as_str(), "post" | "put" | "patch" | "delete") {
                    assert!(operation["responses"].get("403").is_some());
                }
            }
        }
        assert!(
            spec["paths"]["/api/capabilities"]["get"]["responses"]
                .get("401")
                .is_none()
        );
        assert!(
            spec["paths"]["/api/admin/cards/{card_id}"]["patch"]["responses"]
                .get("404")
                .is_some()
        );
        assert!(
            spec["paths"]["/api/drills/hanja"]["get"]["responses"]
                .get("400")
                .is_some()
        );
    }

    #[test]
    fn nullable_response_fields_match_serialized_presence() {
        let spec = serde_json::to_value(ApiDoc::openapi()).unwrap();
        for (name, body) in [
            (
                "AuthResponse",
                serde_json::to_value(crate::auth::AuthResponse {
                    username: None,
                    is_admin: false,
                })
                .unwrap(),
            ),
            (
                "HanjaDrillResponse",
                serde_json::to_value(crate::cards::HanjaDrillResponse { drill: None }).unwrap(),
            ),
            (
                "InflectionHint",
                serde_json::to_value(crate::inflection_hints::InflectionHint::default()).unwrap(),
            ),
        ] {
            let required = spec["components"]["schemas"][name]["required"]
                .as_array()
                .unwrap();
            for key in body.as_object().unwrap().keys() {
                assert!(
                    required.contains(&serde_json::json!(key)),
                    "{name}.{key} is always serialized"
                );
            }
        }
        let mutation_header = |path: &str, method: &str| {
            spec["paths"][path][method]["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .any(|parameter| parameter["name"] == "X-Mutation-Id")
        };
        assert!(mutation_header("/api/user/settings", "patch"));
        assert!(!mutation_header("/api/reviews/check-freshness", "post"));
        assert!(!mutation_header("/api/auth/login", "post"));
    }

    #[test]
    fn passkey_ceremonies_describe_their_wire_envelopes() {
        let spec = serde_json::to_value(ApiDoc::openapi()).unwrap();
        for prefix in [
            "/api/auth/passkey/register",
            "/api/auth/passkey/login",
            "/api/user/passkeys/register",
        ] {
            let start = &spec["paths"][format!("{prefix}/start")]["post"];
            assert_eq!(
                start["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
                "#/components/schemas/StartResponse"
            );
            let finish = &spec["paths"][format!("{prefix}/finish")]["post"];
            assert_eq!(finish["requestBody"]["required"], true);
            assert_eq!(
                finish["requestBody"]["content"]["application/json"]["schema"]["$ref"],
                "#/components/schemas/PasskeyFinishRequest"
            );
        }
        for (schema, payload) in [
            ("StartResponse", "options"),
            ("PasskeyFinishRequest", "credential"),
        ] {
            let required = spec["components"]["schemas"][schema]["required"]
                .as_array()
                .unwrap();
            assert!(required.contains(&serde_json::json!("ceremony_id")));
            assert!(required.contains(&serde_json::json!(payload)));
        }
    }
}
