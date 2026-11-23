use super::*;

struct PendingSave {
    reader: BufReader<TcpStream>,
    body: Vec<u8>,
}

impl PendingSave {
    fn begin(port: u16, token: &str, generation: u64, exposure: f32) -> Self {
        let body = serde_json::to_vec(&json!({
            "id": 0,
            "recipe": {"exposure_ev": exposure},
        }))
        .unwrap();
        let mut socket = TcpStream::connect(("127.0.0.1", port)).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        write!(
            socket,
            "POST /api/xmp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\
             X-AutoShade-Token: {token}\r\nX-AutoShade-Gen: {generation}\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\n\
             Expect: 100-continue\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        let mut reader = BufReader::new(socket);
        assert_eq!(read_http_head(&mut reader), 100);
        Self { reader, body }
    }

    fn finish(mut self) {
        self.reader.get_mut().write_all(&self.body).unwrap();
        assert_eq!(read_http_head(&mut self.reader), 200);
    }
}

#[test]
#[allow(clippy::disallowed_methods)] // a ninth real client must wait independently of this test thread
fn a_ninth_request_waits_for_one_of_eight_active_requests_and_then_recovers() {
    let tree = TempTree::new("http-request-slots");
    let photo = fixture(&tree);
    let (server, port) = start_server(&tree, photo.parent().unwrap());
    let token = session_token(port);
    let generation = listing_generation(port);
    let mut active: Vec<PendingSave> = (0..8)
        .map(|index| PendingSave::begin(port, &token, generation, index as f32 * 0.1))
        .collect();
    let (done_tx, done_rx) = mpsc::channel();

    let waiter = std::thread::spawn(move || {
        let status = request(port, "GET", "/api/list")
            .call()
            .map(|response| response.status());
        let _ = done_tx.send(status);
    });
    match done_rx.recv_timeout(Duration::from_millis(500)) {
        Err(mpsc::RecvTimeoutError::Timeout) => {}
        early => {
            drop(server);
            let _ = waiter.join();
            panic!("the ninth request ran before any active request completed: {early:?}");
        }
    }

    active.remove(0).finish();
    let recovered = done_rx.recv_timeout(Duration::from_secs(5));
    if !matches!(&recovered, Ok(Ok(200))) {
        drop(server);
        let _ = waiter.join();
        panic!("a released request slot did not wake the waiting request: {recovered:?}");
    }
    waiter.join().unwrap();
    for request in active.drain(..) {
        request.finish();
    }
}

fn cache_control(response: &ureq::Response) -> String {
    response
        .header("Cache-Control")
        .expect("the response declares its cache policy")
        .to_ascii_lowercase()
}

#[test]
fn a_pre_save_recipe_miss_cannot_be_reused_after_the_photo_is_saved() {
    let tree = TempTree::new("http-recipe-cache");
    let photo = fixture(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());

    let missing = response(request(port, "GET", "/api/recipe?id=0").call());
    assert_eq!(missing.status(), 404);
    assert!(cache_control(&missing).contains("no-store"));

    save(port, &session_token(port), listing_generation(port), 1.25);
    let saved = request(port, "GET", "/api/recipe?id=0").call().unwrap();
    assert_eq!(saved.status(), 200);
    assert!(cache_control(&saved).contains("no-store"));
    let recipe: Value = saved.into_json().unwrap();
    assert_eq!(recipe["exposure_ev"], 1.25);
}

#[test]
fn the_ui_document_refuses_framing_and_stale_reuse() {
    let tree = TempTree::new("http-ui-headers");
    let photo = fixture(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());

    let page = request(port, "GET", "/").call().unwrap();
    let policy = cache_control(&page);

    assert_eq!(page.header("X-Frame-Options"), Some("DENY"));
    for directive in ["no-cache", "no-store", "must-revalidate"] {
        assert!(policy.contains(directive), "missing {directive}: {policy}");
    }
}
