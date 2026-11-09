//! HTTP mutation adapters: publish only after the underlying operation commits.
//! Keep domain handlers usable without a live-event service (tests/other callers).
//! Each successful operation publishes one descriptive event; failed operations
//! publish none. A review/settings/import event already conveys its queue impact.
use super::Change;
use crate::{
    admin,
    auth::{AdminUser, AuthUser},
    cards,
    error::{AppError, AppJson, AppPath},
    export_import,
    passkey::AppState,
    user,
};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};

pub async fn check_answer(
    State(state): State<AppState>,
    path: AppPath<i64>,
    auth: AuthUser,
    headers: HeaderMap,
    payload: AppJson<cards::CheckRequest>,
) -> Result<Json<cards::CheckResponse>, AppError> {
    let user_id = auth.0;
    let change = Change::CardReviewed {
        card_id: path.0,
        review_revision: payload.0.review_revision.saturating_add(1),
    };
    let response = cards::check_answer(State(state.pool), path, auth, payload).await?;
    state.events.publish(Some(user_id), change, &headers);
    Ok(response)
}

pub async fn suppress_card(
    State(state): State<AppState>,
    path: AppPath<i64>,
    auth: AuthUser,
    headers: HeaderMap,
) -> Result<Json<cards::ReviewResponse>, AppError> {
    let user_id = auth.0;
    let change = Change::QueueChanged {
        card_id: path.0,
        suppressed: true,
    };
    let response = cards::suppress_card(State(state.pool), path, auth).await?;
    state.events.publish(Some(user_id), change, &headers);
    Ok(response)
}
pub async fn unsuppress_card(
    State(state): State<AppState>,
    path: AppPath<i64>,
    auth: AuthUser,
    headers: HeaderMap,
) -> Result<Json<cards::ReviewResponse>, AppError> {
    let user_id = auth.0;
    let change = Change::QueueChanged {
        card_id: path.0,
        suppressed: false,
    };
    let response = cards::unsuppress_card(State(state.pool), path, auth).await?;
    state.events.publish(Some(user_id), change, &headers);
    Ok(response)
}
pub async fn update_settings(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    payload: AppJson<user::UpdateSettingsRequest>,
) -> Result<Json<user::UpdateSettingsResponse>, AppError> {
    let user_id = auth.0;
    let queue_affected = payload.0.study_timezone.is_some()
        || payload.0.initial_study_timezone.is_some()
        || payload.0.day_boundary_hour.is_some()
        || payload.0.daily_new_card_limit.is_some()
        || payload.0.desired_retention.is_some();
    let response = user::update_settings(State(state.pool), auth, payload).await?;
    state.events.publish(
        Some(user_id),
        Change::SettingsChanged { queue_affected },
        &headers,
    );
    Ok(response)
}
pub async fn import_data(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    payload: AppJson<export_import::ImportDataRequest>,
) -> Result<Json<export_import::ImportDataResponse>, AppError> {
    let user_id = auth.0;
    let response = export_import::import_data(State(state.pool), auth, payload).await?;
    state
        .events
        .publish(Some(user_id), Change::StateReset, &headers);
    Ok(response)
}
pub async fn edit_card(
    admin: AdminUser,
    State(state): State<AppState>,
    path: AppPath<i64>,
    headers: HeaderMap,
    payload: AppJson<admin::UpdateCardRequest>,
) -> Result<Json<admin::EditCardResponse>, AppError> {
    let change = Change::CardUpdated { card_id: path.0 };
    let response = admin::edit_card(admin, State(state.pool), path, payload).await?;
    state.events.publish(None, change, &headers);
    Ok(response)
}
pub async fn optimize_fsrs(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
) -> Result<Json<cards::OptimizeFsrsResponse>, AppError> {
    let user_id = auth.0;
    let response = cards::optimize_fsrs(State(state.pool), auth).await?;
    state.events.publish(
        Some(user_id),
        Change::SettingsChanged {
            queue_affected: true,
        },
        &headers,
    );
    Ok(response)
}
pub async fn reset_fsrs_parameters(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    let user_id = auth.0;
    let response = cards::reset_fsrs_parameters(State(state.pool), auth).await?;
    state.events.publish(
        Some(user_id),
        Change::SettingsChanged {
            queue_affected: true,
        },
        &headers,
    );
    Ok(response)
}
