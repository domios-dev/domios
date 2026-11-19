use super::*;

async fn serve_replies(listener: &tokio::net::TcpListener) {
    let catalogue = json!({
        "all":[{
            "id":"upstream-a", "name":"Fixture provider",
            "models":{"beam-free":{"name":"x".repeat(MAX_RUNTIME_VALUE_BYTES)}}
        }],
        "connected":["upstream-a"], "default":{"upstream-a":"beam-free"}
    })
    .to_string();
    let bodies = [
        catalogue.as_str(),
        catalogue.as_str(),
        "",
        "",
        r#"{"ready":true}"#,
    ];
    for (index, body) in bodies.into_iter().enumerate() {
        let (socket, _) = listener.accept().await.unwrap();
        let mut socket = BufReader::new(socket);
        let mut line = String::new();
        socket.read_line(&mut line).await.unwrap();
        assert_eq!(line, "GET /provider HTTP/1.1\r\n");
        loop {
            line.clear();
            assert_ne!(socket.read_line(&mut line).await.unwrap(), 0);
            if line == "\r\n" {
                break;
            }
        }
        let stream = socket.get_mut();
        let headers = match index {
            2 => format!("Content-Length: {}\r\n", RESPONSE_BODY_LIMIT + 1),
            3 => "Transfer-Encoding: chunked\r\n".into(),
            _ => format!("Content-Length: {}\r\n", body.len()),
        };
        stream
            .write_all(format!("HTTP/1.1 200 OK\r\n{headers}Connection: close\r\n\r\n").as_bytes())
            .await
            .unwrap();
        if index == 3 {
            let chunk = vec![b' '; 1024 * 1024];
            for _ in 0..RESPONSE_BODY_LIMIT / chunk.len() {
                stream.write_all(b"100000\r\n").await.unwrap();
                stream.write_all(&chunk).await.unwrap();
                stream.write_all(b"\r\n").await.unwrap();
            }
            stream.write_all(b"1\r\n \r\n0\r\n\r\n").await.unwrap();
        } else if !body.is_empty() {
            stream.write_all(body.as_bytes()).await.unwrap();
        }
    }
}

#[tokio::test]
async fn finite_reply_limits_are_request_local_and_keep_projected_catalogues() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let declaration = serde_json::from_value(json!({
        "readinessPrefix":"fixture listening on ", "passwordEnv":"FIXTURE_PASSWORD",
        "username":"fixture", "usernameEnv":"FIXTURE_USERNAME",
        "eventPath":"/event", "eventMethod":"fixture/event",
        "routes":{"providers":{"method":"GET","path":"/provider"}}
    }))
    .unwrap();
    let (client, mut messages) = Client::channel(None, 16, 16);
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let pending = Mutex::new(requests::Requests::default());
    let read = async {
        for (index, params) in [
            json!({"responseFields":{"all":["id","name"],"connected":[]}}),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
        ]
        .into_iter()
        .enumerate()
        {
            let call = RpcCall {
                method: "providers".into(),
                params,
                target: None,
            };
            let response = client.request_guarded(&call, None);
            tokio::pin!(response);
            let write = tokio::select! {
                write = messages.recv() => write.unwrap(),
                reply = &mut response => panic!("reply before HTTP request: {reply:?}"),
            };
            execute(
                &client,
                &http,
                &origin,
                &declaration,
                "fixture",
                &pending,
                write,
            )
            .await
            .unwrap();
            let reply = response.await.unwrap();
            match index {
                0 => assert_eq!(
                    reply.unwrap(),
                    json!({"body":{"all":[{"id":"upstream-a","name":"Fixture provider"}],"connected":["upstream-a"]},"headers":{}})
                ),
                4 => assert_eq!(reply.unwrap()["body"], json!({"ready":true})),
                _ => {
                    let error = reply.unwrap_err();
                    assert_eq!(error.code, RESPONSE_TOO_LARGE);
                    let (boundary, limit) = if index == 1 {
                        ("runtime_value", MAX_RUNTIME_VALUE_BYTES)
                    } else {
                        ("transport_body", RESPONSE_BODY_LIMIT)
                    };
                    assert_eq!(
                        error.data,
                        Some(json!({
                            "kind":"response_too_large","boundary":boundary,"limitBytes":limit
                        }))
                    );
                }
            }
            assert!(client.ended().borrow().is_none());
        }
    };
    tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(serve_replies(&listener), read);
    })
    .await
    .unwrap();
}
