use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[test]
fn review_and_answer_use_nested_queue_and_signed_attempt_token() {
    let directory = std::env::temp_dir().join(format!(
        "hwaiting_review_contract_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        for path in [
            "post /api/auth/login ",
            "get /api/cards/queue ",
            "post /api/cards/991/check ",
            "put /api/cards/991/suppress ",
        ] {
            let (mut socket, _) = listener.accept().unwrap();
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
            assert!(headers.starts_with(path), "{headers}");
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
            let (body, cookie) = if path.contains("login") {
                (
                    json!({"username":"test","is_admin":true}),
                    "Set-Cookie: hwaiting-auth=test-jwt; HttpOnly; Path=/; Max-Age=3600\r\n",
                )
            } else {
                assert!(headers.contains("cookie: hwaiting-auth=test-jwt"));
                let body = if path.contains("queue") {
                    json!({"cards":[{"card":{"card_id":991,"grammar_pattern":{"slug":"realization","name":"Realization","explanation":"Authored content"}},"review":{"attempt_token":"signed-attempt","hanja_hint_words":[]}}],"next_due_at":null,"refresh_at":"2026-11-23T04:00:00Z"})
                } else {
                    assert!(headers.contains("x-requested-with: hwaiting"));
                    if path.contains("check") {
                        assert_eq!(
                            serde_json::from_slice::<Value>(&data[end..end + length]).unwrap(),
                            json!({"answer":"answer","attempt_token":"signed-attempt"})
                        );
                        json!({"correct":true,"hanja_hints":[]})
                    } else {
                        json!({"success":true})
                    }
                };
                (body, "")
            };
            let body = body.to_string();
            write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n{cookie}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
    });
    let run = |args: &[&str]| {
        let result = Command::new(env!("CARGO_BIN_EXE_hwaiting-agent"))
            .args(args)
            .env("HWAITING_API_URL", &base)
            .env("XDG_CONFIG_HOME", &directory)
            .env("HWAITING_USERNAME", "test")
            .env("HWAITING_PASSWORD", "test-password")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        serde_json::from_slice::<Value>(&result.stdout).unwrap()
    };
    run(&["login"]);
    let entry = run(&["review"]);
    assert_eq!(entry["card"]["grammar_pattern"]["name"], "Realization");
    assert_eq!(entry["review"]["attempt_token"], "signed-attempt");
    assert!(entry["review"].get("hanja_hint_words").is_none());
    assert_eq!(
        run(&["answer", "991", "answer", "signed-attempt"])["correct"],
        true
    );
    server.join().unwrap();
    fs::remove_dir_all(directory).unwrap();
}
