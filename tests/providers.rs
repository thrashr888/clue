use clue::{
    Candidate,
    provider::{Options, Provider, apply_ollama},
};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    process::Command,
    thread,
    time::Duration,
};

fn items() -> Vec<Candidate> {
    serde_json::from_value(json!([
        {"id":"private-id-a","title":"Groceries","location":"/private/path"},
        {"id":"private-id-b","title":"Offline notebook sync","record":{"secret":"local-only"}}
    ]))
    .unwrap()
}

fn server(responses: Vec<Value>) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let mut requests = vec![];
        for response in responses {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut bytes = vec![];
            let mut buf = [0; 4096];
            loop {
                let n = stream.read(&mut buf).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buf[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                    let size: usize = headers
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length: "))
                        .unwrap()
                        .parse()
                        .unwrap();
                    if bytes.len() >= end + 4 + size {
                        break;
                    }
                }
            }
            requests.push(String::from_utf8(bytes).unwrap());
            let body = response.to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        }
        requests
    });
    (url, handle)
}

#[test]
fn routing_is_explicit_and_remote_content_requires_consent() {
    for url in [
        "http://127.0.0.1:8009",
        "http://localhost:8009",
        "http://[::1]:8009",
    ] {
        assert!(
            Options {
                provider: Provider::Systemone,
                model: None,
                base_url: Some(url.into())
            }
            .resolve(false)
            .is_ok()
        );
    }
    for url in [
        "https://models.example.org",
        "http://127.0.0.1.evil.example",
        "http://user:secret@localhost:8009",
        "http://localhost:8009?key=secret",
        "http://localhost:8009/v1",
    ] {
        assert!(
            Options {
                provider: Provider::Systemone,
                model: None,
                base_url: Some(url.into())
            }
            .resolve(false)
            .is_err()
        );
    }
    assert!(
        Options {
            provider: Provider::Typesafe,
            model: None,
            base_url: Some("http://localhost:8009".into())
        }
        .resolve(true)
        .is_err()
    );
    assert!(
        Options {
            provider: Provider::Ollama,
            model: None,
            base_url: None
        }
        .resolve(false)
        .is_err()
    );
}

#[test]
fn generated_ratings_preserve_identity_without_fabricating_probabilities() {
    let mut data = items();
    apply_ollama(
        &json!({"done":true,"message":{"content":"{\"r0\":0,\"r1\":3}"}}),
        &mut data,
    )
    .unwrap();
    assert_eq!(data[0].id, "private-id-b");
    let result = serde_json::to_value(&data[0]).unwrap();
    assert_eq!(result["relevance"]["kind"], "generated_rating");
    assert!(result["relevance"].get("probabilities").is_none());
    assert!(result["relevance"].get("confidence").is_none());
    assert_eq!(result["record"]["secret"], "local-only");
    for content in [
        "{\"r0\":0}",
        "{\"r0\":0,\"r1\":4}",
        "{\"r0\":0,\"r1\":2.5}",
        "{\"r0\":0,\"r2\":3}",
        "{\"r0\":0,\"r1\":3,\"r2\":1}",
        "```json\n{}\n```",
    ] {
        let before = serde_json::to_value(&data).unwrap();
        assert!(
            apply_ollama(
                &json!({"done":true,"message":{"content":content}}),
                &mut data
            )
            .is_err()
        );
        assert_eq!(before, serde_json::to_value(&data).unwrap());
    }
    assert!(apply_ollama(&json!({"done":true,"done_reason":"length","message":{"content":"{\"r0\":0,\"r1\":3}"}}), &mut data).is_err());
}

#[test]
fn systemone_http_uses_explicit_provider_key_never_typesafe_credentials() {
    let answer = |n| {
        json!({"type":"score","score":n,"confidence":1.0,"probabilities":{
        "0":if n==0 {1.0}else{0.0},"1":0.0,"2":0.0,"3":if n==3 {1.0}else{0.0}}})
    };
    let (url, handle) = server(vec![
        json!({"model":"kev-test","answers":{"r0":answer(0),"r1":answer(3)}}),
    ]);
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("input.json");
    std::fs::write(&input, serde_json::to_vec(&items()).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_clue"))
        .args([
            "rank",
            "notebook sync",
            "--provider",
            "systemone",
            "--base-url",
            &url,
            "--input",
            input.to_str().unwrap(),
        ])
        .env("TYPESAFE_API_KEY", "do-not-forward")
        .env("CLUE_PROVIDER_API_KEY", "local-server-key")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let d: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(d["api"]["provider"], "systemone");
    assert_eq!(d["results"][0]["id"], "private-id-b");
    let requests = handle.join().unwrap();
    assert!(requests[0].starts_with("POST /v1/systemone "));
    assert!(requests[0].contains("Bearer local-server-key"));
    for forbidden in [
        "do-not-forward",
        "private-id",
        "/private/path",
        "local-only",
    ] {
        assert!(!requests[0].contains(forbidden));
    }
}

#[tokio::test]
async fn ollama_http_uses_schema_and_refuses_unapproved_cloud_model() {
    let (url, handle) = server(vec![
        json!({"capabilities":["completion"]}),
        json!({"model":"local-test","done":true,"message":{"content":"{\"r0\":0,\"r1\":3}"}}),
    ]);
    let config = Options {
        provider: Provider::Ollama,
        model: Some("local-test".into()),
        base_url: Some(url),
    }
    .resolve(false)
    .unwrap();
    config
        .rank("sync", &mut items(), Duration::from_secs(3), false)
        .await
        .unwrap();
    let requests = handle.join().unwrap();
    assert!(requests[0].starts_with("POST /api/show "));
    assert!(requests[1].starts_with("POST /api/chat "));
    assert!(requests[1].contains("additionalProperties"));
    assert!(!requests[1].contains("private-id"));
    let (url, handle) = server(vec![
        json!({"remote_host":"https://ollama.com","remote_model":"cloud"}),
    ]);
    let config = Options {
        provider: Provider::Ollama,
        model: Some("apparently-local".into()),
        base_url: Some(url),
    }
    .resolve(false)
    .unwrap();
    assert!(
        config
            .rank("sync", &mut items(), Duration::from_secs(3), false)
            .await
            .unwrap_err()
            .to_string()
            .contains("--share-content")
    );
    assert_eq!(handle.join().unwrap().len(), 1);
}

#[tokio::test]
async fn unreachable_local_server_does_not_fall_back_to_jev() {
    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", socket.local_addr().unwrap());
    drop(socket);
    let config = Options {
        provider: Provider::Systemone,
        model: None,
        base_url: Some(url),
    }
    .resolve(false)
    .unwrap();
    let mut data = items();
    assert!(
        config
            .rank("sync", &mut data, Duration::from_secs(1), false)
            .await
            .is_err()
    );
    assert!(data.iter().all(|i| i.relevance.is_none()));
}

#[cfg(unix)]
#[test]
fn local_ranking_is_available_in_search_context_and_sqlite() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let cider = temp.path().join("cider");
    std::fs::write(&cider, "#!/bin/sh\nprintf '%s' '[{\"id\":\"a\",\"title\":\"sync one\",\"completed\":false},{\"id\":\"b\",\"title\":\"sync two\",\"completed\":false}]'").unwrap();
    std::fs::set_permissions(&cider, std::fs::Permissions::from_mode(0o700)).unwrap();
    let db = temp.path().join("test.db");
    rusqlite::Connection::open(&db).unwrap().execute_batch("CREATE TABLE tasks(id TEXT PRIMARY KEY,title TEXT); INSERT INTO tasks VALUES('a','sync one'),('b','sync two');").unwrap();
    for command in [
        vec!["search", "sync", "--sources", "reminders", "--ai"],
        vec!["context", "sync", "--sources", "reminders", "--ai"],
        vec![
            "sqlite",
            "search",
            db.to_str().unwrap(),
            "sync",
            "--table",
            "tasks",
            "--columns",
            "title",
            "--ai",
        ],
    ] {
        let answer = json!({"type":"score","score":3,"confidence":1,"probabilities":{"0":0,"1":0,"2":0,"3":1}});
        let (url, handle) = server(vec![
            json!({"model":"local-test","answers":{"r0":answer,"r1":answer}}),
        ]);
        let output = Command::new(env!("CARGO_BIN_EXE_clue"))
            .args([
                "--provider",
                "systemone",
                "--base-url",
                &url,
                "--cider",
                cider.to_str().unwrap(),
            ])
            .args(command)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let d: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(d["ai"]["applied"], true, "{d}");
        assert_eq!(d["ai"]["metadata"]["provider"], "systemone");
        assert_eq!(d["mode"], "systemone_reranked");
        handle.join().unwrap();
    }
}

#[tokio::test]
async fn ollama_rejects_input_that_would_exceed_reported_context() {
    let (url, handle) = server(vec![json!({"model_info":{"test.context_length":1024}})]);
    let config = Options {
        provider: Provider::Ollama,
        model: Some("tiny-context".into()),
        base_url: Some(url),
    }
    .resolve(false)
    .unwrap();
    assert!(
        config
            .rank("sync", &mut items(), Duration::from_secs(3), false)
            .await
            .unwrap_err()
            .to_string()
            .contains("context budget")
    );
    assert_eq!(handle.join().unwrap().len(), 1);
}
