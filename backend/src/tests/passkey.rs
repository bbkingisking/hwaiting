use super::*;

#[tokio::test]
async fn labels_are_owner_scoped_validated_and_can_be_cleared() {
    let pool = crate::test_support::test_pool().await;
    let owner = crate::test_support::test_user(&pool).await;
    let other = crate::test_support::test_user(&pool).await;
    let state = AppState {
        events: crate::events::EventHub::default(),
        pool: pool.clone(),
        webauthn: None,
        ceremonies: Arc::new(Mutex::new(HashMap::new())),
    };
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO passkeys (user_id, public_key) VALUES (?, X'01') RETURNING id",
    )
    .bind(owner)
    .fetch_one(&pool)
    .await
    .unwrap();
    let update = |user, id, label: &str| {
        update_passkey(
            State(state.clone()),
            AuthUser(user),
            AppPath(id),
            AppJson(UpdatePasskeyRequest {
                label: label.to_string(),
            }),
        )
    };
    let initial = list_passkeys(State(state.clone()), AuthUser(owner))
        .await
        .unwrap()
        .0;
    assert!(initial.passkeys[0].label.is_none());
    assert!(matches!(
        update(other, id, "Someone else's key").await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        update(owner, id + 1, "Missing").await,
        Err(AppError::NotFound)
    ));

    let renamed = update(owner, id, "  내 노트북  ").await.unwrap().0;
    assert_eq!(renamed.label.as_deref(), Some("내 노트북"));
    assert_eq!(renamed.created_at, initial.passkeys[0].created_at);
    assert_eq!(renamed.last_used_at, initial.passkeys[0].last_used_at);
    let listed = list_passkeys(State(state.clone()), AuthUser(owner))
        .await
        .unwrap()
        .0;
    assert_eq!(listed.passkeys[0].label, renamed.label);
    assert!(
        list_passkeys(State(state.clone()), AuthUser(other))
            .await
            .unwrap()
            .0
            .passkeys
            .is_empty()
    );

    for invalid in [
        "a".repeat(101),
        "line\nbreak".to_string(),
        "null\0byte".to_string(),
    ] {
        assert!(matches!(
            update(owner, id, &invalid).await,
            Err(AppError::BadRequest(_))
        ));
    }
    let unchanged = list_passkeys(State(state.clone()), AuthUser(owner))
        .await
        .unwrap()
        .0;
    assert_eq!(unchanged.passkeys[0].label, renamed.label);
    assert!(update(owner, id, &"한".repeat(100)).await.is_ok());
    assert!(update(owner, id, "   ").await.unwrap().0.label.is_none());
    let key: Vec<u8> = sqlx::query_scalar("SELECT public_key FROM passkeys WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(key, vec![1]);
}

#[test]
fn public_key_storage_contains_only_the_algorithm_and_key() {
    use webauthn_rp::response::register::Ed25519PubKey;

    // RFC 8032, section 7.1, test 1 public key.
    let bytes = [
        0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07,
        0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07,
        0x51, 0x1a,
    ];
    let key = UncompressedPubKey::Ed25519(Ed25519PubKey::try_from(bytes.as_slice()).unwrap());
    let encoded = encode_public_key(&key);
    assert_eq!(encoded.len(), 33);
    assert_eq!(encoded[0], 0);
    assert_eq!(&encoded[1..], &bytes);
    let StoredPubKey::Ed25519(decoded) = decode_public_key(&encoded).unwrap() else {
        panic!("wrong public key algorithm");
    };
    assert_eq!(decoded.into_inner(), bytes);

    for len in 0..encoded.len() {
        assert!(decode_public_key(&encoded[..len]).is_err());
    }
    let mut unknown_algorithm = encoded.clone();
    unknown_algorithm[0] = 255;
    assert!(decode_public_key(&unknown_algorithm).is_err());

    let old_format = StaticState {
        credential_public_key: key,
        extensions: AuthenticatorExtensionOutputStaticState {
            cred_protect: CredentialProtectionPolicy::UserVerificationRequired,
            hmac_secret: None,
        },
    }
    .encode()
    .unwrap();
    assert!(decode_public_key(&old_format).is_err());
}

#[test]
fn ec_and_rsa_storage_preserves_key_material_and_rejects_malformed_data() {
    use webauthn_rp::response::register::{
        RsaPubKey, UncompressedP256PubKey, UncompressedP384PubKey,
    };

    // Serialization fixtures only; curve/signature validation belongs to
    // the WebAuthn verifier. Exercise both possible EC y parities.
    for parity in [0, 1] {
        let x256 = [0x42; 32];
        let y256 = [parity; 32];
        let x384 = [0x43; 48];
        let y384 = [parity; 48];
        let keys = [
            UncompressedPubKey::P256(
                UncompressedP256PubKey::try_from((x256.as_slice(), y256.as_slice())).unwrap(),
            ),
            UncompressedPubKey::P384(
                UncompressedP384PubKey::try_from((x384.as_slice(), y384.as_slice())).unwrap(),
            ),
        ];
        for key in keys {
            let mut encoded = encode_public_key(&key);
            match decode_public_key(&encoded).unwrap() {
                StoredPubKey::P256(key) => {
                    assert_eq!(key.x(), &x256);
                    assert_eq!(key.y_is_odd(), parity == 1);
                    assert_eq!(encoded.len(), 34);
                }
                StoredPubKey::P384(key) => {
                    assert_eq!(key.x(), &x384);
                    assert_eq!(key.y_is_odd(), parity == 1);
                    assert_eq!(encoded.len(), 50);
                }
                _ => panic!("wrong key algorithm"),
            }
            for len in 0..encoded.len() {
                assert!(decode_public_key(&encoded[..len]).is_err());
            }
            encoded[1] = 4;
            assert!(decode_public_key(&encoded).is_err());
            encoded[1] = 2;
            encoded.push(0);
            assert!(decode_public_key(&encoded).is_err());
        }
    }

    let modulus = [0xff; 256];
    let key = UncompressedPubKey::Rsa(RsaPubKey::try_from((modulus.as_slice(), 65537)).unwrap());
    let mut encoded = encode_public_key(&key);
    let StoredPubKey::Rsa(decoded) = decode_public_key(&encoded).unwrap() else {
        panic!("wrong key algorithm");
    };
    assert_eq!(decoded.n(), &modulus);
    assert_eq!(decoded.e(), 65537);
    for len in 0..encoded.len() {
        assert!(decode_public_key(&encoded[..len]).is_err());
    }
    encoded.push(0);
    assert!(decode_public_key(&encoded).is_err());
}

#[tokio::test]
async fn registration_ceremonies_are_bound_to_their_endpoint_and_account() {
    let pool = crate::test_support::test_pool().await;
    let owner = crate::test_support::test_user(&pool).await;
    let other = crate::test_support::test_user(&pool).await;
    let state = AppState {
        events: crate::events::EventHub::default(),
        pool: pool.clone(),
        webauthn: Some(Arc::new(WebauthnConfig {
            rp_id: RpId::Domain(AsciiDomain::try_from("localhost".to_string()).unwrap()),
            origins: vec!["http://localhost".to_string()],
        })),
        ceremonies: Arc::new(Mutex::new(HashMap::new())),
    };
    let before_users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();

    let add = add_passkey_start(State(state.clone()), AuthUser(owner))
        .await
        .unwrap()
        .0;
    assert!(matches!(
        take_registration_for(
            &state,
            add.ceremony_id,
            RegistrationPurpose::AddToAccount(other)
        ),
        Err(AppError::Forbidden)
    ));
    assert!(matches!(
        take_registration_for(
            &state,
            add.ceremony_id,
            RegistrationPurpose::AddToAccount(owner)
        ),
        Err(AppError::CeremonyNotFound)
    ));

    let add = add_passkey_start(State(state.clone()), AuthUser(owner))
        .await
        .unwrap()
        .0;
    assert!(matches!(
        take_registration_for(&state, add.ceremony_id, RegistrationPurpose::NewAccount),
        Err(AppError::CeremonyNotFound)
    ));

    let public = register_start(State(state.clone())).await.unwrap().0;
    assert_eq!(
        public.options["authenticatorSelection"]["userVerification"],
        "required"
    );
    assert!(public.options["extensions"]["prf"].is_null());
    assert!(matches!(
        take_registration_for(
            &state,
            public.ceremony_id,
            RegistrationPurpose::AddToAccount(owner)
        ),
        Err(AppError::Forbidden)
    ));

    let add = add_passkey_start(State(state.clone()), AuthUser(owner))
        .await
        .unwrap()
        .0;
    assert!(
        take_registration_for(
            &state,
            add.ceremony_id,
            RegistrationPurpose::AddToAccount(owner)
        )
        .is_ok()
    );
    let public = register_start(State(state.clone())).await.unwrap().0;
    assert!(
        take_registration_for(&state, public.ceremony_id, RegistrationPurpose::NewAccount).is_ok()
    );

    // These are real, generated WebAuthn challenges. A forbidden finish
    // is rejected at this gate, before credential verification or SQL.
    let after_users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    let passkeys: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM passkeys")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(after_users, before_users);
    assert_eq!(passkeys, 0);
}

#[tokio::test]
async fn concurrent_deletions_preserve_a_passwordless_sign_in_method() {
    let pool = crate::test_support::test_pool().await;
    let user_id = crate::test_support::test_user(&pool).await;
    let ids: Vec<i64> = sqlx::query_scalar(
        "INSERT INTO passkeys (user_id, public_key) VALUES (?, X'01'), (?, X'02') RETURNING id",
    )
    .bind(user_id)
    .bind(user_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    let state = AppState {
        events: crate::events::EventHub::default(),
        pool: pool.clone(),
        webauthn: None,
        ceremonies: Arc::new(Mutex::new(HashMap::new())),
    };

    let (first, second) = tokio::join!(
        delete_passkey(State(state.clone()), AuthUser(user_id), AppPath(ids[0])),
        delete_passkey(State(state), AuthUser(user_id), AppPath(ids[1])),
    );
    let results = [first, second];
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Ok(StatusCode::NO_CONTENT)))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(AppError::BadRequest(_))))
            .count(),
        1
    );
    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM passkeys WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(remaining, 1);
}

#[test]
fn accepts_plain_https_origin() {
    assert_eq!(
        parse_origin("https://hwaiting.example.com").unwrap(),
        "https://hwaiting.example.com"
    );
}

#[test]
fn accepts_http_origin() {
    assert_eq!(
        parse_origin("http://localhost").unwrap(),
        "http://localhost"
    );
}

#[test]
fn accepts_explicit_port() {
    assert_eq!(
        parse_origin("http://localhost:5173").unwrap(),
        "http://localhost:5173"
    );
}

#[test]
fn strips_trailing_slash() {
    assert_eq!(
        parse_origin("https://hwaiting.example.com/").unwrap(),
        "https://hwaiting.example.com"
    );
}

#[test]
fn rejects_other_schemes() {
    assert!(parse_origin("ftp://hwaiting.example.com").is_err());
}

#[test]
fn rejects_a_path() {
    assert!(parse_origin("https://hwaiting.example.com/login").is_err());
}

#[test]
fn rejects_a_query_string() {
    assert!(parse_origin("https://hwaiting.example.com?x=1").is_err());
}

#[test]
fn rejects_a_fragment() {
    assert!(parse_origin("https://hwaiting.example.com#top").is_err());
}

#[test]
fn rejects_userinfo() {
    assert!(parse_origin("https://user:pass@hwaiting.example.com").is_err());
}

// No separate "bare hostname, no scheme" test: `Url::parse` fails on a
// schemeless string the same way it fails on any other unparseable
// input, so this would hit the exact same early return as
// rejects_unparseable_input below rather than the scheme-check branch.

#[test]
fn rejects_unparseable_input() {
    assert!(parse_origin("not a url at all").is_err());
}

#[test]
fn does_not_lowercase_the_host() {
    // Documents current behavior: no case-normalization happens, even
    // though a real browser always sends a lowercase origin - an
    // operator who configures HWAITING_RP_ORIGINS with any uppercase
    // letters in the host will get an origin that a real browser
    // request can never match by exact string equality (see
    // WebauthnConfig's doc comment on why comparison is a plain string
    // compare).
    assert_eq!(
        parse_origin("https://HWaiting.Example.com").unwrap(),
        "https://HWaiting.Example.com"
    );
}
