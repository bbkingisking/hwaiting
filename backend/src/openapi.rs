use utoipa::{
    openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
    Modify, OpenApi,
};

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearer_auth",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .bearer_format("JWT")
                        .build(),
                ),
            );
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "hwaiting API",
        version = "1.0.0",
        description = "Korean FSRS flashcard backend. Stateless JWT bearer auth \
            (Authorization: Bearer <token>), obtained from /api/auth/login or \
            /api/auth/signup. Every error response is {\"error\": \"<message>\"}. \
            See CORS_ALLOWED_ORIGINS / STATIC_DIR / JWT_EXPIRY_SECONDS env vars \
            for deployment-time behavior not visible in this spec."
    ),
    paths(
        crate::auth::login,
        crate::auth::signup,
        crate::cards::get_next_card,
        crate::cards::list_enum_lookups,
        crate::cards::check_answer,
        crate::cards::comment_on_card,
        crate::cards::suppress_card,
        crate::cards::unsuppress_card,
        crate::cards::list_suppressed_cards,
        crate::cards::get_stats,
        crate::cards::get_review_history,
        crate::cards::get_history_summary,
        crate::cards::get_history_breakdown,
        crate::cards::optimize_fsrs,
        crate::cards::reset_fsrs_parameters,
        crate::user::get_profile,
        crate::user::get_settings,
        crate::user::update_settings,
        crate::export_import::export_data,
        crate::export_import::import_data,
        crate::custom_cards::create_custom_card,
        crate::custom_cards::list_custom_cards,
        crate::custom_cards::get_custom_card,
        crate::custom_cards::update_custom_card,
        crate::custom_cards::delete_custom_card,
        crate::admin::list_invites,
        crate::admin::generate_invites,
        crate::admin::delete_invite,
        crate::admin::search_cards,
        crate::admin::edit_card,
        crate::health_check,
    ),
    components(schemas(
        crate::error::ErrorResponse,
        crate::auth::LoginRequest,
        crate::auth::SignupRequest,
        crate::auth::AuthResponse,
        crate::cards::EnumLookups,
        crate::cards::NextCardEnvelope,
        crate::cards::CheckRequest,
        crate::cards::CheckResponse,
        crate::cards::CommentRequest,
        crate::cards::CommentResponse,
        crate::cards::ReviewResponse,
        crate::cards::StatsResponse,
        crate::cards::SuppressedCardsResponse,
        crate::cards::ReviewHistoryResponse,
        crate::cards::HistorySummary,
        crate::cards::OptimizeFsrsResponse,
        crate::cards::HistoryBreakdownResponse,
        crate::user::UserProfile,
        crate::user::UserSettings,
        crate::user::UpdateSettingsRequest,
        crate::user::UpdateSettingsResponse,
        crate::export_import::ExportData,
        crate::export_import::ImportDataRequest,
        crate::export_import::ImportDataResponse,
        crate::custom_cards::CreateCustomCardRequest,
        crate::custom_cards::CreateCustomCardResponse,
        crate::custom_cards::ListCustomCardsResponse,
        crate::custom_cards::CustomCard,
        crate::custom_cards::UpdateCustomCardRequest,
        crate::custom_cards::UpdateCustomCardResponse,
        crate::admin::GenerateInvitesRequest,
        crate::admin::GenerateInvitesResponse,
        crate::admin::ListInvitesResponse,
        crate::admin::SearchCardsResponse,
    )),
    modifiers(&SecurityAddon),
    tags(
        (name = "auth", description = "Signup/login, no auth required"),
        (name = "cards", description = "Review flow, FSRS scheduling, stats"),
        (name = "user", description = "Profile, settings, data export/import"),
        (name = "custom-cards", description = "User-authored cards, scoped to the owner"),
        (name = "admin", description = "Requires is_admin = true on the JWT's user"),
        (name = "misc", description = "Health check"),
    )
)]
pub struct ApiDoc;
