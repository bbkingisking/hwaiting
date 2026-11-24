use utoipa::{
    Modify, OpenApi,
    openapi::security::{ApiKey, ApiKeyValue, SecurityRequirement, SecurityScheme},
    openapi::{
        Ref, Required,
        content::ContentBuilder,
        path::{ParameterBuilder, ParameterIn},
        response::ResponseBuilder,
        schema::{AllOfBuilder, ObjectBuilder, OneOfBuilder, Type},
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
            // One SSE event on /api/events: the event name, and the
            // Notification serialized as JSON in `data`.
            components.schemas.insert(
                "NotificationEvent".into(),
                ["card-mutated", "state-changed", "resync-required"]
                    .into_iter()
                    .fold(
                        OneOfBuilder::new().description(Some(
                            "One server-sent event: `event` equals the notification `type` in the JSON carried by `data`.",
                        )),
                        |events, kind| {
                            events.item(
                                ObjectBuilder::new()
                                    .property(
                                        "event",
                                        ObjectBuilder::new()
                                            .schema_type(Type::String)
                                            .enum_values(Some([kind])),
                                    )
                                    .property(
                                        "data",
                                        ObjectBuilder::new()
                                            .schema_type(Type::String)
                                            .content_media_type("application/json")
                                            .content_schema(Some(
                                                AllOfBuilder::new()
                                                    .item(Ref::from_schema_name("Notification"))
                                                    .item(
                                                        ObjectBuilder::new()
                                                            .property(
                                                                "type",
                                                                ObjectBuilder::new()
                                                                    .schema_type(Type::String)
                                                                    .enum_values(Some([kind])),
                                                            )
                                                            .required("type"),
                                                    ),
                                            )),
                                    )
                                    .required("event")
                                    .required("data"),
                            )
                        },
                    )
                    .into(),
            );
            // Translation maps are keyed by language code; utoipa only types keys as strings.
            for name in ["AdminSentence", "AdminCardDetail"] {
                if let Some(utoipa::openapi::RefOr::T(utoipa::openapi::schema::Schema::Object(
                    object,
                ))) = components.schemas.get_mut(name)
                    && let Some(utoipa::openapi::RefOr::T(utoipa::openapi::schema::Schema::Object(
                        map,
                    ))) = object.properties.get_mut("translations")
                {
                    map.property_names = Some(Box::new(
                        ObjectBuilder::new()
                            .schema_type(Type::String)
                            .pattern(Some("^[a-z]{3}$"))
                            .into(),
                    ));
                }
            }
            components.add_security_scheme(
                "cookie_auth",
                SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new(
                    crate::auth_cookie::COOKIE_NAME,
                ))),
            );
        }
        if let Some(content) = openapi
            .paths
            .paths
            .get_mut("/api/events")
            .and_then(|path| path.get.as_mut())
            .and_then(|operation| operation.responses.responses.get_mut("200"))
            .and_then(|response| match response {
                utoipa::openapi::RefOr::T(response) => {
                    response.content.get_mut("text/event-stream")
                }
                utoipa::openapi::RefOr::Ref(_) => None,
            })
            .and_then(|content| match content {
                utoipa::openapi::RefOr::T(content) => Some(content),
                utoipa::openapi::RefOr::Ref(_) => None,
            })
        {
            content.item_schema = Some(Ref::from_schema_name("NotificationEvent").into());
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
                // Logout and health touch no fallible state, so they cannot fail with 500.
                if !matches!(
                    operation.operation_id.as_deref(),
                    Some("logout" | "health_check")
                ) {
                    operation
                        .responses
                        .responses
                        .entry("500".into())
                        .or_insert_with(|| error_response("Internal server error"));
                }
                // Path parameters are parsed by AppPath, which rejects malformed values.
                let has_path_parameter = operation.parameters.iter().flatten().any(|parameter| {
                    matches!(
                        parameter,
                        utoipa::openapi::RefOr::T(parameter)
                            if parameter.parameter_in == ParameterIn::Path
                    )
                });
                if has_path_parameter {
                    operation
                        .responses
                        .responses
                        .entry("400".into())
                        .or_insert_with(|| error_response("Malformed path parameter"));
                }
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
                            error_response(
                                "Missing, invalid, or expired authentication cookie (`unauthenticated`)",
                            )
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
                        .schema(Some(
                            ObjectBuilder::new()
                                .schema_type(Type::String)
                                .format(Some(utoipa::openapi::schema::SchemaFormat::Custom(
                                    "uuid".into(),
                                ))),
                        ))
                        .build()
                        .into(),
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
                        .build()
                        .into(),
                );
            }
        }
        extract_shared_components(openapi);
    }
}

/// Share only identical response objects: status codes alone do not identify
/// errors, and endpoint-specific descriptions must retain their meaning.
fn extract_shared_components(openapi: &mut utoipa::openapi::OpenApi) {
    let components = openapi.components.get_or_insert_with(Default::default);
    for (name, description) in [
        ("InternalServerError", "Internal server error"),
        (
            "Unauthenticated",
            "Missing, invalid, or expired authentication cookie (`unauthenticated`)",
        ),
        (
            "CsrfHeaderRequired",
            "Missing or invalid X-Requested-With header",
        ),
        ("MalformedPathParameter", "Malformed path parameter"),
        (
            "PasskeysUnavailable",
            "Passkeys are not configured on this deployment",
        ),
        (
            "PasskeyVerificationFailed",
            "Ceremony verification failed, or unknown ceremony id",
        ),
        ("PasskeyCeremonyExpired", "Ceremony expired"),
    ] {
        components
            .responses
            .insert(name.into(), error_response(description));
    }
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
            for parameter in operation.parameters.iter_mut().flatten() {
                let utoipa::openapi::RefOr::T(value) = parameter else {
                    continue;
                };
                if value.parameter_in != ParameterIn::Header {
                    continue;
                }
                let name = match value.name.as_str() {
                    "X-Requested-With" => "RequestedWith",
                    "X-Mutation-Id" => "MutationId",
                    _ => continue,
                };
                let reference = Ref::new(format!("#/components/parameters/{name}"));
                let definition =
                    std::mem::replace(parameter, utoipa::openapi::RefOr::Ref(reference));
                if let Some(existing) = components.parameters.get(name) {
                    assert!(
                        existing == &definition,
                        "shared header definitions must agree"
                    );
                } else {
                    components.parameters.insert(name.into(), definition);
                }
            }
            for response in operation.responses.responses.values_mut() {
                if let Some((name, _)) = components
                    .responses
                    .iter()
                    .find(|(_, shared)| *shared == response)
                {
                    *response = Ref::new(format!("#/components/responses/{name}")).into();
                }
            }
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    version = "3.2.0",
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
        crate::classifications::list_classifications,
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

    fn resolved_spec() -> serde_json::Value {
        let mut spec = serde_json::to_value(ApiDoc::openapi()).unwrap();
        let definitions = spec.clone();
        fn resolve(value: &mut serde_json::Value, definitions: &serde_json::Value) {
            match value {
                serde_json::Value::Object(object) => {
                    if let Some(reference) = object.get("$ref").and_then(|v| v.as_str())
                        && (reference.starts_with("#/components/parameters/")
                            || reference.starts_with("#/components/responses/"))
                    {
                        *value = definitions
                            .pointer(&reference[1..])
                            .expect("valid component reference")
                            .clone();
                        return;
                    }
                    for child in object.values_mut() {
                        resolve(child, definitions);
                    }
                }
                serde_json::Value::Array(array) => {
                    for child in array {
                        resolve(child, definitions);
                    }
                }
                _ => {}
            }
        }
        resolve(&mut spec, &definitions);
        spec
    }

    #[test]
    fn review_and_stream_examples_are_consistent() {
        let spec = resolved_spec();
        let queue = &spec["paths"]["/api/reviews/queue"]["get"]["responses"]["200"]["content"]["application/json"]
            ["examples"]["available"]["value"];
        let answer = &spec["paths"]["/api/reviews/answer"]["post"];
        let request = &answer["requestBody"]["content"]["application/json"]["example"];
        let response = &answer["responses"]["200"]["content"]["application/json"]["example"];
        let card = &queue["cards"][0]["card"];
        assert_eq!(
            queue["cards"][0]["review"]["attempt_token"],
            request["attempt_token"]
        );
        assert_eq!(request["answer"], response["target"]);
        assert_eq!(
            format!(
                "{}{}{}",
                card["sentence_before"].as_str().unwrap(),
                response["target"].as_str().unwrap(),
                card["sentence_after"].as_str().unwrap()
            ),
            response["sentence"].as_str().unwrap()
        );
        for withheld in ["word", "sentence", "target", "alternatives"] {
            assert!(
                card.get(withheld).is_none(),
                "queue example reveals {withheld}"
            );
        }
        let stream =
            spec["paths"]["/api/events"]["get"]["responses"]["200"]["content"]["text/event-stream"]
                ["example"]
                .as_str()
                .unwrap();
        let mut count = 0;
        for event in stream.split("\n\n").filter(|event| !event.is_empty()) {
            let (name, data) = event.split_once("\ndata: ").unwrap();
            let notification: serde_json::Value = serde_json::from_str(data).unwrap();
            assert_eq!(name.strip_prefix("event: ").unwrap(), notification["type"]);
            count += 1;
        }
        assert_eq!(count, 3);
    }

    #[test]
    fn shared_components_reuse_headers_and_common_errors() {
        let spec = serde_json::to_value(ApiDoc::openapi()).unwrap();
        let answer = &spec["paths"]["/api/reviews/answer"]["post"];
        for name in ["RequestedWith", "MutationId"] {
            assert!(
                answer["parameters"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| p["$ref"] == format!("#/components/parameters/{name}"))
            );
        }
        assert_eq!(
            answer["responses"]["500"]["$ref"],
            "#/components/responses/InternalServerError"
        );
        assert_eq!(
            answer["responses"]["401"]["$ref"],
            "#/components/responses/Unauthenticated"
        );
        assert_eq!(
            spec["paths"]["/api/auth/login"]["post"]["responses"]["401"]["description"],
            "Invalid credentials"
        );
        let parameters = &spec["components"]["parameters"];
        assert_eq!(parameters["RequestedWith"]["required"], true);
        assert_eq!(
            parameters["RequestedWith"]["schema"]["enum"],
            serde_json::json!(["hwaiting"])
        );
        assert_eq!(parameters["MutationId"]["required"], false);
        assert_eq!(parameters["MutationId"]["schema"]["format"], "uuid");
    }

    #[test]
    fn document_is_openapi_3_2_with_typed_event_stream() {
        let spec = resolved_spec();
        assert_eq!(spec["openapi"], "3.2.0");
        let mutation_id = spec["paths"]["/api/reviews/answer"]["post"]["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "X-Mutation-Id")
            .unwrap();
        assert_eq!(mutation_id["schema"]["format"], "uuid");
        let schemas = &spec["components"]["schemas"];
        for (schema, property) in [
            ("AdminSentence", "translations"),
            ("AdminCardDetail", "translations"),
        ] {
            assert_eq!(
                schemas[schema]["properties"][property]["propertyNames"]["pattern"], "^[a-z]{3}$",
                "{schema}.{property} keys"
            );
        }
        let stream = &spec["paths"]["/api/events"]["get"]["responses"]["200"]["content"]["text/event-stream"];
        assert_eq!(
            stream["itemSchema"]["$ref"],
            "#/components/schemas/NotificationEvent"
        );
        let event = &spec["components"]["schemas"]["NotificationEvent"];
        let branches = event["oneOf"].as_array().unwrap();
        let notification_variants = schemas["Notification"]["oneOf"].as_array().unwrap();
        assert_eq!(branches.len(), notification_variants.len());
        // Each event name permits only the matching decoded payload type, while
        // retaining the full Notification constraints (e.g. card_id is required).
        for variant in notification_variants {
            let kind = &variant["properties"]["type"]["enum"][0];
            let matching: Vec<_> = branches
                .iter()
                .filter(|branch| branch["properties"]["event"]["enum"] == serde_json::json!([kind]))
                .collect();
            assert_eq!(
                matching.len(),
                1,
                "{kind} must have exactly one event branch"
            );
            let branch = matching[0];
            assert_eq!(branch["required"], serde_json::json!(["event", "data"]));
            let data = &branch["properties"]["data"];
            assert_eq!(data["type"], "string");
            assert_eq!(data["contentMediaType"], "application/json");
            let payload = &data["contentSchema"]["allOf"];
            assert_eq!(payload[0]["$ref"], "#/components/schemas/Notification");
            assert_eq!(payload[1]["required"], serde_json::json!(["type"]));
            assert_eq!(
                payload[1]["properties"]["type"]["enum"],
                serde_json::json!([kind])
            );
        }
        // OpenAPI requires a description on every response object.
        for (path, operations) in spec["paths"].as_object().unwrap() {
            for (method, operation) in operations.as_object().unwrap() {
                for (status, response) in operation["responses"].as_object().unwrap() {
                    assert!(
                        response["description"]
                            .as_str()
                            .is_some_and(|d| !d.is_empty()),
                        "{method} {path} {status} has no description"
                    );
                }
            }
        }
    }

    #[test]
    fn shared_failures_match_authentication_and_write_middleware() {
        let spec = resolved_spec();
        for item in spec["paths"].as_object().unwrap().values() {
            for (method, operation) in item.as_object().unwrap() {
                let infallible = matches!(
                    operation["operationId"].as_str(),
                    Some("logout" | "health_check")
                );
                assert_eq!(operation["responses"].get("500").is_none(), infallible);
                if operation.get("security").is_some() {
                    assert_eq!(
                        operation["responses"]["401"]["description"],
                        "Missing, invalid, or expired authentication cookie (`unauthenticated`)"
                    );
                }
                let has_path_parameter = operation["parameters"]
                    .as_array()
                    .is_some_and(|parameters| parameters.iter().any(|p| p["in"] == "path"));
                if has_path_parameter {
                    assert!(operation["responses"].get("400").is_some());
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
        let spec = resolved_spec();
        for (name, body) in [
            (
                "UserProfile",
                serde_json::to_value(crate::user::UserProfile {
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
        let spec = resolved_spec();
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
