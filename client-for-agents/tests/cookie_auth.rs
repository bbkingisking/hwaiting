use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[test]
fn login_persists_private_scoped_cookies_for_the_next_process() {
    let directory = std::env::temp_dir().join(format!(
        "hwaiting_cookie_test_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    let state = directory.join("hwaiting-agent");
    fs::create_dir_all(&state).unwrap();
    fs::write(state.join("token"), "legacy-token").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());

    let server = std::thread::spawn(move || {
        for is_login in [true, false] {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                let read = socket.read(&mut buffer).unwrap();
                assert_ne!(read, 0);
                request.extend_from_slice(&buffer[..read]);
            }
            let header_end = request
                .windows(4)
                .position(|part| part == b"\r\n\r\n")
                .unwrap()
                + 4;
            let header_text = String::from_utf8(request[..header_end].to_vec())
                .unwrap()
                .to_ascii_lowercase();
            let content_length: usize = header_text
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .unwrap_or("0")
                .parse()
                .unwrap();
            while request.len() < header_end + content_length {
                let read = socket.read(&mut buffer).unwrap();
                assert_ne!(read, 0);
                request.extend_from_slice(&buffer[..read]);
            }
            let request = header_text;
            assert!(!request.contains("authorization:"));
            let body = if is_login {
                assert!(request.starts_with("post /api/auth/login "));
                assert!(request.contains("x-requested-with: hwaiting"));
                r#"{"username":"test","is_admin":false}"#
            } else {
                assert!(request.starts_with("get /api/cards/field-values?"));
                assert!(request.contains("cookie: hwaiting-auth=test-jwt"));
                "{}"
            };
            let cookie = if is_login {
                "Set-Cookie: hwaiting-auth=test-jwt; HttpOnly; SameSite=Lax; Path=/; Max-Age=3600\r\n"
            } else {
                ""
            };
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n{cookie}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });

    let run = |command: &str, url: &str| {
        Command::new(env!("CARGO_BIN_EXE_hwaiting-agent"))
            .arg(command)
            .env("HWAITING_API_URL", url)
            .env("XDG_CONFIG_HOME", &directory)
            .env("HWAITING_USERNAME", "test")
            .env("HWAITING_PASSWORD", "test-password")
            .output()
            .unwrap()
    };
    let login = run("login", &base_url);
    assert!(
        login.status.success(),
        "{}",
        String::from_utf8_lossy(&login.stderr)
    );
    assert!(!String::from_utf8_lossy(&login.stdout).contains("test-jwt"));
    assert!(!state.join("token").exists());
    assert_eq!(
        fs::metadata(state.join("cookies.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let fields = run("field-values", &base_url);
    assert!(
        fields.status.success(),
        "{}",
        String::from_utf8_lossy(&fields.stderr)
    );
    server.join().unwrap();

    let different_server = run("field-values", "http://127.0.0.1:1");
    assert!(!different_server.status.success());
    assert!(String::from_utf8_lossy(&different_server.stderr).contains("different API URL"));
    fs::remove_dir_all(directory).unwrap();
}
