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
        description = "Korean FSRS flashcard backend."
    ),
    paths(
        crate::auth::login,
        crate::auth::signup,
        crate::cards::get_next_card,
        crate::cards::list_field_values,
        crate::cards::check_answer,
        crate::cards::comment_on_card,
        crate::cards::suppress_card,
        crate::cards::unsuppress_card,
        crate::cards::list_suppressed_cards,
        crate::cards::get_stats,
        crate::cards::get_history,
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
        crate::admin::list_users,
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
        crate::cards::FieldValues,
        crate::cards::FieldName,
        crate::cards::NextCardEnvelope,
        crate::cards::CheckRequest,
        crate::cards::CheckResponse,
        crate::cards::CommentRequest,
        crate::cards::CommentResponse,
        crate::cards::ReviewResponse,
        crate::cards::StatsResponse,
        crate::cards::SuppressedCardsResponse,
        crate::cards::HistoryResponse,
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
        crate::admin::AdminUserSummary,
        crate::admin::ListUsersResponse,
        crate::admin::GenerateInvitesRequest,
        crate::admin::GenerateInvitesResponse,
        crate::admin::ListInvitesResponse,
        crate::admin::SearchCardsResponse,
        crate::admin::UpdateCardRequest,
        crate::admin::EditCardResponse,
    )),
    modifiers(&SecurityAddon),
    tags(
        (name = "auth", description = "Invite code-gated sign-up and simple login"),
        (name = "cards", description = "Review flow, FSRS scheduling, stats"),
        (name = "user", description = "Profile, settings, data export/import"),
        (name = "custom-cards", description = "User-authored cards, scoped to the owner"),
        (name = "admin", description = "Requires is_admin = true on the JWT's user"),
        (name = "misc", description = "Health check"),
    )
)]
// Convention for every list-shaped response registered above (ListUsersResponse,
// ListInvitesResponse, GenerateInvitesResponse, ListCustomCardsResponse,
// SearchCardsResponse, SuppressedCardsResponse, ...): the collection lives in a
// field named after the resource, plural - `users`, `codes`, `cards` - never a
// generic `items`/`data` key. Keep new list endpoints consistent with that
// rather than introducing another wrapper name.
pub struct ApiDoc;
