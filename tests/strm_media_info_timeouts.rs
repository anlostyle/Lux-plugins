use std::{
    io::{BufRead, BufReader, Write},
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Stdio},
};

use serde_json::{Value, json};

const PROBE_JSON: &str = r#"{"format":{"format_name":"mov,mp4","duration":"10.0","size":"100","bit_rate":"1000"},"streams":[{"index":0,"codec_type":"video","codec_name":"h264","width":1920,"height":1080}]}"#;

fn scratch_dir(label: &str) -> std::path::PathBuf {
    let path =
        std::env::temp_dir().join(format!("lux-strm-timeouts-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&path).expect("scratch dir");
    path
}

fn fake_ffprobe(directory: &Path, sleep_seconds: u32) -> std::path::PathBuf {
    let path = directory.join("ffprobe");
    std::fs::write(
        &path,
        format!("#!/bin/sh\nsleep {sleep_seconds}\nprintf '%s' '{PROBE_JSON}'\n"),
    )
    .expect("fake ffprobe");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

fn media_probe(ffprobe: &Path, params: Value) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_lux-plugin-strm-media-info"))
        .env("LUX_FFPROBE_BINARY", ffprobe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn plugin");
    let request = json!({"id": "1", "method": "media.probe", "params": params});
    let mut stdin = child.stdin.take().expect("stdin");
    writeln!(stdin, "{request}").expect("write request");
    let mut line = String::new();
    BufReader::new(child.stdout.take().expect("stdout"))
        .read_line(&mut line)
        .expect("read response");
    drop(stdin);
    let _ = child.wait();
    serde_json::from_str(&line).expect("response json")
}

#[test]
fn a_slow_ffprobe_times_out_at_the_configured_limit_and_succeeds_with_a_larger_one() {
    let directory = scratch_dir("slow");
    let ffprobe = fake_ffprobe(&directory, 3);

    let timed_out = media_probe(
        &ffprobe,
        json!({"url": "/cloud/a.mp4", "includeMediaInfo": true, "ffprobeTimeoutSeconds": 1}),
    );
    assert_eq!(timed_out["error"]["code"], "MEDIA_PROBE_TIMEOUT");

    let succeeded = media_probe(
        &ffprobe,
        json!({"url": "/cloud/a.mp4", "includeMediaInfo": true, "ffprobeTimeoutSeconds": 10}),
    );
    assert!(
        succeeded["error"].is_null(),
        "unexpected error: {succeeded}"
    );
    assert!(succeeded["result"].is_object());
}

#[test]
fn the_default_limit_still_applies_when_the_host_sends_no_timeout() {
    let directory = scratch_dir("default");
    let ffprobe = fake_ffprobe(&directory, 1);
    let response = media_probe(
        &ffprobe,
        json!({"url": "/cloud/a.mp4", "includeMediaInfo": true}),
    );
    assert!(response["error"].is_null(), "unexpected error: {response}");
}

#[test]
fn manifest_declares_validated_timeout_settings() {
    let manifest: Value =
        serde_json::from_str(include_str!("../manifests/org.lux.strm-media-info.json"))
            .expect("manifest json");
    let fields = manifest["configFields"].as_array().expect("configFields");
    for (key, default, max) in [
        ("ffprobeTimeoutSeconds", 30, 600),
        ("ffmpegTimeoutSeconds", 60, 900),
    ] {
        let field = fields
            .iter()
            .find(|field| field["key"] == key)
            .unwrap_or_else(|| panic!("{key}"));
        assert_eq!(field["type"], "number");
        assert_eq!(field["defaultValue"], default);
        assert_eq!(field["minimum"], 10);
        assert_eq!(field["maximum"], max);
        assert!(
            field["description"]
                .as_str()
                .is_some_and(|text| text.contains("网盘"))
        );
    }
}
