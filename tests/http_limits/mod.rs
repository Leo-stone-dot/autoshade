use super::*;

const JSON_LIMIT: u64 = 256 * 1024 * 1024;
const UPLOAD_LIMIT: u64 = 500 * 1024 * 1024;

fn declared_post(port: u16, path: &str, token: &str, generation: u64, length: u64) -> u16 {
    let mut socket = TcpStream::connect(("127.0.0.1", port)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(
        socket,
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\
         X-AutoShade-Token: {token}\r\nX-AutoShade-Gen: {generation}\r\n\
         X-Filename: photo.png\r\nContent-Length: {length}\r\n\
         Expect: 100-continue\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    read_http_head(&mut BufReader::new(socket))
}

#[test]
fn a_declared_oversized_save_is_refused_before_uploading_the_body() {
    let tree = TempTree::new("http-json-length");
    let photo = fixture(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let token = session_token(port);
    let generation = listing_generation(port);
    save(port, &token, generation, 0.75);
    let before = saved(port);

    let status = declared_post(port, "/api/xmp", &token, generation, JSON_LIMIT + 1);

    assert_eq!(
        status, 400,
        "oversized JSON must be refused without requesting its body"
    );
    assert_eq!(saved(port), before);
}

#[test]
fn a_declared_oversized_upload_is_refused_without_adding_a_photo() {
    let tree = TempTree::new("http-upload-length");
    let photo = fixture(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let before = gallery(port);

    let status = declared_post(
        port,
        "/api/upload",
        &session_token(port),
        listing_generation(port),
        UPLOAD_LIMIT + 1,
    );

    assert_eq!(
        status, 413,
        "oversized uploads must be refused without requesting their body"
    );
    assert_eq!(gallery(port), before);
}

fn padded_save(length: u64) -> impl Read {
    let json = br#"{"id":0,"recipe":{"exposure_ev":1.25}}"#;
    assert!(length >= json.len() as u64);
    Cursor::new(json).chain(std::io::repeat(b' ').take(length - json.len() as u64))
}

#[test]
#[ignore = "streams 256 MiB into a real server: cargo test --release --test serve_behavior http_limits::a_json_save_at_the_documented_limit_is_accepted -- --ignored --exact"]
fn a_json_save_at_the_documented_limit_is_accepted() {
    let tree = TempTree::new("http-json-at-limit");
    let photo = fixture(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let result = save_request(port, &session_token(port), listing_generation(port))
        .timeout(Duration::from_secs(90))
        .set("Content-Length", &JSON_LIMIT.to_string())
        .send(padded_save(JSON_LIMIT))
        .unwrap();

    assert_eq!(result.status(), 200);
    assert_eq!(saved(port).0["exposure_ev"], 1.25);
}

#[test]
#[ignore = "streams over 256 MiB chunked: cargo test --release --test serve_behavior http_limits::a_chunked_save_over_the_limit_cannot_replace_an_existing_edit -- --ignored --exact"]
fn a_chunked_save_over_the_limit_cannot_replace_an_existing_edit() {
    let tree = TempTree::new("http-json-chunked-limit");
    let photo = fixture(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let token = session_token(port);
    let generation = listing_generation(port);
    save(port, &token, generation, 0.75);
    let before = saved(port);

    let result = response(
        save_request(port, &token, generation)
            .timeout(Duration::from_secs(90))
            .set("Transfer-Encoding", "chunked")
            .send(padded_save(JSON_LIMIT + 1)),
    );

    assert_eq!(result.status(), 400);
    assert!(result.into_string().unwrap().contains("exceeds 256 MiB"));
    assert_eq!(saved(port), before);
}
