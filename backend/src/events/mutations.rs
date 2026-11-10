// HTTP mutation adapters publish one notification after success.
// Domain handlers remain usable without a live-event service; failures emit none.
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
    let card_id = path.0;
    let response = cards::check_answer(State(state.pool), path, auth, payload).await?;
    state.events.publish(Some(user_id), Some(card_id), &headers);
    Ok(response)
}

pub async fn suppress_card(
    State(state): State<AppState>,
    path: AppPath<i64>,
    auth: AuthUser,
    headers: HeaderMap,
) -> Result<Json<cards::ReviewResponse>, AppError> {
    let user_id = auth.0;
    let card_id = path.0;
    let response = cards::suppress_card(State(state.pool), path, auth).await?;
    state.events.publish(Some(user_id), Some(card_id), &headers);
    Ok(response)
}
pub async fn unsuppress_card(
    State(state): State<AppState>,
    path: AppPath<i64>,
    auth: AuthUser,
    headers: HeaderMap,
) -> Result<Json<cards::ReviewResponse>, AppError> {
    let user_id = auth.0;
    let card_id = path.0;
    let response = cards::unsuppress_card(State(state.pool), path, auth).await?;
    state.events.publish(Some(user_id), Some(card_id), &headers);
    Ok(response)
}
pub async fn update_settings(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    payload: AppJson<user::UpdateSettingsRequest>,
) -> Result<Json<user::UpdateSettingsResponse>, AppError> {
    let user_id = auth.0;
    let response = user::update_settings(State(state.pool), auth, payload).await?;
    state.events.publish(Some(user_id), None, &headers);
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
    state.events.publish(Some(user_id), None, &headers);
    Ok(response)
}
pub async fn edit_card(
    admin: AdminUser,
    State(state): State<AppState>,
    path: AppPath<i64>,
    headers: HeaderMap,
    payload: AppJson<admin::UpdateCardRequest>,
) -> Result<Json<admin::EditCardResponse>, AppError> {
    let card_id = path.0;
    let response = admin::edit_card(admin, State(state.pool), path, payload).await?;
    if response.changed {
        state.events.publish(None, Some(card_id), &headers);
    }
    Ok(response)
}
pub async fn optimize_fsrs(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
) -> Result<Json<cards::OptimizeFsrsResponse>, AppError> {
    let user_id = auth.0;
    let response = cards::optimize_fsrs(State(state.pool), auth).await?;
    state.events.publish(Some(user_id), None, &headers);
    Ok(response)
}
pub async fn reset_fsrs_parameters(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    let user_id = auth.0;
    let response = cards::reset_fsrs_parameters(State(state.pool), auth).await?;
    state.events.publish(Some(user_id), None, &headers);
    Ok(response)
}
