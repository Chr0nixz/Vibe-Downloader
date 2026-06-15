use tauri_app_lib::{
    db,
    proxy::{normalize_proxy_url, socks5_connect, AppProxyMode, ResolvedProxyConfig},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[test]
fn proxy_url_normalization_rejects_credentials_and_bad_schemes() {
    assert_eq!(
        normalize_proxy_url(" socks5://127.0.0.1:1080 "),
        Some("socks5://127.0.0.1:1080".to_string())
    );
    assert_eq!(
        normalize_proxy_url("http://proxy.local:8080"),
        Some("http://proxy.local:8080/".to_string())
    );
    assert!(normalize_proxy_url("ftp://proxy.local:21").is_none());
    assert!(normalize_proxy_url("socks5://user:pass@127.0.0.1:1080").is_none());
    assert!(normalize_proxy_url("not-a-url").is_none());
}

#[test]
fn custom_socks5_proxy_gets_auth_url_and_fingerprint_without_password() {
    let config = ResolvedProxyConfig {
        mode: AppProxyMode::Custom,
        url: Some("socks5://127.0.0.1:1080/".to_string()),
        no_proxy: Some("localhost,127.0.0.1".to_string()),
        username: Some("user".to_string()),
        password: Some("secret".to_string()),
    };

    assert_eq!(
        config.custom_socks5_url_with_auth().as_deref(),
        Some("socks5://user:secret@127.0.0.1:1080/")
    );
    assert!(!config.fingerprint().contains("secret"));
}

#[test]
fn sftp_task_proxy_overrides_require_socks5() {
    assert!(db::validate_task_proxy_protocol("sftp", "socks5://127.0.0.1:1080").is_ok());
    let error = db::validate_task_proxy_protocol("sftp", "http://proxy.local:8080")
        .expect_err("http proxy should be rejected for sftp");
    assert!(error.contains("proxy_scheme_unsupported_for_task"));
    assert!(error.contains("SFTP tasks only support SOCKS5"));
}

#[tokio::test]
async fn socks5_connect_supports_no_auth() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let proxy_addr = listener.local_addr().expect("local addr");
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept");
        let mut greeting = [0_u8; 3];
        stream.read_exact(&mut greeting).await.expect("greeting");
        assert_eq!(greeting, [0x05, 0x01, 0x00]);
        stream.write_all(&[0x05, 0x00]).await.expect("method");
        assert_connect_request(&mut stream, "example.com", 443).await;
        stream
            .write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0x1f, 0x90])
            .await
            .expect("connect response");
    });

    let stream = socks5_connect(
        &format!("socks5://{proxy_addr}"),
        None,
        None,
        "example.com",
        443,
    )
    .await
    .expect("socks connect");
    drop(stream);
    server.await.expect("server");
}

#[tokio::test]
async fn socks5_connect_supports_username_password() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let proxy_addr = listener.local_addr().expect("local addr");
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept");
        let mut greeting = [0_u8; 3];
        stream.read_exact(&mut greeting).await.expect("greeting");
        assert_eq!(greeting, [0x05, 0x01, 0x02]);
        stream.write_all(&[0x05, 0x02]).await.expect("method");

        let mut auth = [0_u8; 12];
        stream.read_exact(&mut auth).await.expect("auth");
        assert_eq!(
            &auth,
            &[0x01, 0x04, b'u', b's', b'e', b'r', 0x05, b'p', b'a', b's', b's', b'1']
        );
        stream
            .write_all(&[0x01, 0x00])
            .await
            .expect("auth response");

        assert_connect_request(&mut stream, "ftp.example.com", 21).await;
        stream
            .write_all(&[0x05, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00])
            .await
            .expect("connect response");
    });

    let stream = socks5_connect(
        &format!("socks5://{proxy_addr}"),
        Some("user"),
        Some("pass1"),
        "ftp.example.com",
        21,
    )
    .await
    .expect("socks connect");
    drop(stream);
    server.await.expect("server");
}

async fn assert_connect_request(stream: &mut tokio::net::TcpStream, host: &str, port: u16) {
    let mut prefix = [0_u8; 5];
    stream
        .read_exact(&mut prefix)
        .await
        .expect("request prefix");
    assert_eq!(&prefix[..4], &[0x05, 0x01, 0x00, 0x03]);
    let len = prefix[4] as usize;
    let mut host_bytes = vec![0_u8; len];
    stream.read_exact(&mut host_bytes).await.expect("host");
    let mut port_bytes = [0_u8; 2];
    stream.read_exact(&mut port_bytes).await.expect("port");
    assert_eq!(String::from_utf8(host_bytes).expect("utf8"), host);
    assert_eq!(u16::from_be_bytes(port_bytes), port);
}
