use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
    time::Duration,
};

use luxd::application::tmdb::{TmdbClient, TmdbClientConfig};

/// Serves one canned TMDb images response and reports the request line it received.
fn fake_tmdb(body: &'static str) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("address");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut buffer = [0_u8; 4096];
        let read = stream.read(&mut buffer).expect("read");
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        let _ = sender.send(request.lines().next().unwrap_or_default().to_owned());
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).expect("write");
    });
    (format!("http://{address}"), receiver)
}

const HONG_KONG_IMAGES: &str = r#"{"posters":[{"file_path":"/hk.jpg","iso_639_1":"cn","width":500,"height":750},{"file_path":"/en.jpg","iso_639_1":"en","width":500,"height":750}],"backdrops":[]}"#;

#[tokio::test]
async fn default_image_request_includes_the_two_letter_chinese_codes_so_a_cantonese_poster_is_returned()
 {
    let (base_url, request_line) = fake_tmdb(HONG_KONG_IMAGES);
    let client = TmdbClient::new(TmdbClientConfig {
        base_url,
        api_key: Some("test-key".to_owned()),
        timeout: Duration::from_secs(5),
        max_retries: 0,
        ..TmdbClientConfig::default()
    })
    .expect("client");

    let images = client
        .movie_images_for_request(561377, "zh-CN", false)
        .await
        .expect("images");

    let request_line = request_line
        .recv_timeout(Duration::from_secs(5))
        .expect("request");
    assert!(
        request_line.starts_with("GET /3/movie/561377/images?"),
        "{request_line}"
    );
    assert!(request_line.contains("language=zh-CN"), "{request_line}");
    assert!(
        request_line.contains("include_image_language=zh%2Ccn%2Cen%2Cnull"),
        "{request_line}"
    );
    assert!(
        images
            .posters
            .iter()
            .any(|poster| poster.iso_639_1.as_deref() == Some("cn"))
    );
}
