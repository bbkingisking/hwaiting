use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn state_directory() -> PathBuf {
    std::env::temp_dir().join(format!(
        "hwaiting_review_contract_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn read_request(socket: &mut TcpStream) -> (String, Vec<u8>) {
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut data = Vec::new();
    let mut buf = [0; 4096];
    while !data.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = socket.read(&mut buf).unwrap();
        assert_ne!(n, 0);
        data.extend_from_slice(&buf[..n]);
    }
    let end = data.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    let headers = String::from_utf8_lossy(&data[..end]).to_ascii_lowercase();
    let length: usize = headers
        .lines()
        .find_map(|l| l.strip_prefix("content-length: "))
        .unwrap_or("0")
        .parse()
        .unwrap();
    while data.len() < end + length {
        let n = socket.read(&mut buf).unwrap();
        assert_ne!(n, 0);
        data.extend_from_slice(&buf[..n]);
    }
    (headers, data[end..end + length].to_vec())
}

fn respond(socket: &mut TcpStream, body: Value, login: bool) {
    let cookie = if login {
        "Set-Cookie: hwaiting-auth=test-jwt; HttpOnly; Path=/; Max-Age=3600\r\n"
    } else {
        ""
    };
    let body = body.to_string();
    write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n{cookie}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
}

fn serve_login(listener: &TcpListener) {
    let (mut socket, _) = listener.accept().unwrap();
    let (headers, _) = read_request(&mut socket);
    assert!(headers.starts_with("post /api/auth/login "));
    respond(
        &mut socket,
        json!({"username":"test","is_admin":true}),
        true,
    );
}

fn serve_queue(listener: &TcpListener, limit: usize, cards: &[i64]) {
    let (mut socket, _) = listener.accept().unwrap();
    let (headers, _) = read_request(&mut socket);
    assert!(
        headers.starts_with(&format!("get /api/cards/queue?limit={limit} ")),
        "{headers}"
    );
    assert!(headers.contains("cookie: hwaiting-auth=test-jwt"));
    let entries: Vec<Value> = cards
        .iter()
        .map(|id| json!({"card":{"card_id":id},"review":{"attempt_token":"signed-attempt"}}))
        .collect();
    respond(
        &mut socket,
        json!({"cards":entries,"next_due_at":null,"refresh_at":"2026-11-23T04:00:00Z"}),
        false,
    );
}

fn run(base: &str, directory: &Path, args: &[&str]) -> Value {
    let result = Command::new(env!("CARGO_BIN_EXE_hwaiting-agent"))
        .args(args)
        .env("HWAITING_API_URL", base)
        .env("XDG_CONFIG_HOME", directory)
        .env("HWAITING_USERNAME", "test")
        .env("HWAITING_PASSWORD", "test-password")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}

#[test]
fn review_and_answer_use_nested_queue_and_signed_attempt_token() {
    let directory = state_directory();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        serve_login(&listener);
        for path in [
            "get /api/cards/queue?limit=1 ",
            "post /api/cards/991/check ",
            "put /api/cards/991/suppress ",
        ] {
            let (mut socket, _) = listener.accept().unwrap();
            let (headers, body) = read_request(&mut socket);
            assert!(headers.starts_with(path), "{headers}");
            assert!(headers.contains("cookie: hwaiting-auth=test-jwt"));
            let body = if path.contains("queue") {
                json!({"cards":[{"card":{"card_id":991,"grammar_pattern":{"slug":"realization","name":"Realization","explanation":"Authored content"}},"review":{"attempt_token":"signed-attempt","hanja_hint_words":[]}}],"next_due_at":null,"refresh_at":"2026-11-23T04:00:00Z"})
            } else {
                assert!(headers.contains("x-requested-with: hwaiting"));
                if path.contains("check") {
                    assert_eq!(
                        serde_json::from_slice::<Value>(&body).unwrap(),
                        json!({"answer":"answer","attempt_token":"signed-attempt"})
                    );
                    json!({"correct":true,"hanja_hints":[]})
                } else {
                    json!({"success":true})
                }
            };
            respond(&mut socket, body, false);
        }
    });
    run(&base, &directory, &["login"]);
    let entry = run(&base, &directory, &["review"]);
    assert_eq!(entry["card"]["grammar_pattern"]["name"], "Realization");
    assert_eq!(entry["review"]["attempt_token"], "signed-attempt");
    assert!(entry["review"].get("hanja_hint_words").is_none());
    assert_eq!(
        run(
            &base,
            &directory,
            &["answer", "991", "answer", "signed-attempt"]
        )["correct"],
        true
    );
    server.join().unwrap();
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn review_fetches_only_a_prefix_large_enough_to_skip_known_leases() {
    let directory = state_directory();
    let leases = directory.join("hwaiting-agent/leases");
    fs::create_dir_all(&leases).unwrap();
    fs::write(leases.join("991.lease"), "sibling").unwrap();
    fs::write(leases.join("992.lease"), "sibling").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        serve_login(&listener);
        serve_queue(&listener, 3, &[991, 992, 993]);
    });
    run(&base, &directory, &["login"]);
    assert_eq!(run(&base, &directory, &["review"])["card"]["card_id"], 993);
    server.join().unwrap();
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn review_retries_lost_claims_but_stops_when_all_entries_are_already_leased() {
    let directory = state_directory();
    let sibling_directory = directory.clone();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        serve_login(&listener);
        let (mut socket, _) = listener.accept().unwrap();
        let (headers, _) = read_request(&mut socket);
        assert!(headers.starts_with("get /api/cards/queue?limit=1 "));
        // Take the card after the client has read its leases but before it claims.
        fs::write(
            sibling_directory.join("hwaiting-agent/leases/991.lease"),
            "sibling",
        )
        .unwrap();
        respond(
            &mut socket,
            json!({"cards":[{"card":{"card_id":991},"review":{"attempt_token":"signed-attempt"}}]}),
            false,
        );
        serve_queue(&listener, 2, &[991, 992]);
        // A separate review invocation sees both leases and must not keep retrying.
        serve_queue(&listener, 3, &[991, 992]);
    });
    run(&base, &directory, &["login"]);
    assert_eq!(run(&base, &directory, &["review"])["card"]["card_id"], 992);
    let empty = run(&base, &directory, &["review"]);
    assert!(empty["card"].is_null());
    assert_eq!(empty["refresh_at"], "2026-11-23T04:00:00Z");
    server.join().unwrap();
    fs::remove_dir_all(directory).unwrap();
}
