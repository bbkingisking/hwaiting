use utoipa::{
    Modify, OpenApi,
    openapi::security::{ApiKey, ApiKeyValue, SecurityScheme},
    openapi::{
        Required,
        path::{ParameterBuilder, ParameterIn},
        schema::{ObjectBuilder, Type},
    },
};

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "cookie_auth",
                SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new(
                    crate::auth_cookie::COOKIE_NAME,
                ))),
            );
        }
        // Keep the write requirement executable in Swagger UI and visible
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
                operation.parameters.get_or_insert_with(Vec::new).push(
                    ParameterBuilder::new()
                        .name("X-Mutation-Id")
                        .parameter_in(ParameterIn::Header)
                        .required(Required::False)
                        .description(Some("Optional UUID echoed in SSE notifications for this operation; not an identity or idempotency key."))
                        .schema(Some(ObjectBuilder::new().schema_type(Type::String)))
                        .build(),
                );
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
        crate::events::card_state,
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
        crate::cards::get_review_queue,
        crate::cards::list_field_values,
        crate::cards::check_answer,
        crate::cards::comment_on_card,
        crate::cards::suppress_card,
        crate::cards::unsuppress_card,
        crate::cards::list_suppressed_cards,
        crate::cards::get_hanja_drill,
        crate::cards::get_stats,
        crate::cards::get_history,
        crate::cards::optimize_fsrs,
        crate::cards::reset_fsrs_parameters,
        crate::user::get_profile,
        crate::user::get_settings,
        crate::user::update_settings,
        crate::export_import::export_data,
        crate::export_import::import_data,
        crate::admin::list_users,
        crate::admin::search_cards,
        crate::admin::get_card_inflections,
        crate::admin::edit_card,
        crate::health_check,
    ),
    components(schemas(
        crate::events::Change,
        crate::events::CardState,
        crate::error::ErrorResponse,
        crate::error::StaleAnswerResponse,
        crate::auth::LoginRequest,
        crate::auth::SignupRequest,
        crate::auth::AuthResponse,
        crate::passkey::PasskeySummary,
        crate::passkey::ListPasskeysResponse,
        crate::passkey::Capabilities,
        crate::cards::FieldValues,
        crate::cards::FieldName,
        crate::cards::ReviewQueueResponse,
        crate::cards::CheckRequest,
        crate::cards::CheckResponse,
        crate::cards::CommentRequest,
        crate::cards::CommentResponse,
        crate::cards::ReviewResponse,
        crate::cards::StatsResponse,
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
        crate::user::UpdateSettingsResponse,
        crate::export_import::ExportData,
        crate::export_import::WrongAnswerExport,
        crate::export_import::ImportDataRequest,
        crate::export_import::ImportDataResponse,
        crate::admin::AdminUserSummary,
        crate::admin::ListUsersResponse,
        crate::admin::SearchCardsResponse,
        crate::admin::CardInflectionsResponse,
        crate::admin::UpdateCardRequest,
        crate::admin::EditCardResponse,
    )),
    modifiers(&SecurityAddon),
    tags(
        (name = "auth", description = "Open sign-up and login, via either username/password \
            or passkey (WebAuthn) - two independent ways to reach the same JWT"),
        (name = "cards", description = "Review flow, FSRS scheduling, stats"),
        (name = "user", description = "Profile, settings, data export/import"),
        (name = "admin", description = "Requires is_admin = true on the JWT's user"),
        (name = "misc", description = "Health check"),
    )
)]
// Convention for every list-shaped response registered above (ListUsersResponse,
// SearchCardsResponse, SuppressedCardsResponse, ...): the collection lives in a
// field named after the resource, plural - `users`, `codes`, `cards` - never a
// generic `items`/`data` key. Keep new list endpoints consistent with that
// rather than introducing another wrapper name.
pub struct ApiDoc;
