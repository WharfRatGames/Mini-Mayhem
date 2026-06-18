/// Minimal HTTPS helper using rustls — no async, pure std::net + rustls Stream.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use rustls::ClientConfig;
use rustls::pki_types::ServerName;

pub fn make_tls_config() -> Arc<ClientConfig> {
    let root_store = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    Arc::new(
        ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth(),
    )
}

fn tls_roundtrip(host: &str, request: &[u8], connect_timeout: u64, read_timeout: u64) -> Result<Vec<u8>, String> {
    let addr = (host, 443u16)
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("no addr")?;

    let tcp = TcpStream::connect_timeout(&addr, Duration::from_secs(connect_timeout))
        .map_err(|e| e.to_string())?;
    tcp.set_read_timeout(Some(Duration::from_secs(read_timeout))).ok();

    let config = make_tls_config();
    let server_name: ServerName<'static> = ServerName::try_from(host.to_string())
        .map_err(|e| e.to_string())?;
    let mut conn = rustls::ClientConnection::new(config, server_name)
        .map_err(|e| e.to_string())?;
    let mut tcp = tcp;
    let mut stream = rustls::Stream::new(&mut conn, &mut tcp);

    stream.write_all(request).map_err(|e| e.to_string())?;

    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    if resp.is_empty() { return Err("empty response".into()); }

    let body_start = resp.windows(4).position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
        .or_else(|| resp.windows(2).position(|w| w == b"\n\n").map(|i| i + 2))
        .ok_or("no header separator")?;

    Ok(resp[body_start..].to_vec())
}

pub fn https_get(host: &str, path: &str, connect_timeout: u64, read_timeout: u64) -> Result<Vec<u8>, String> {
    let req = format!("GET {} HTTP/1.0\r\nHost: {}\r\nConnection: close\r\n\r\n", path, host);
    tls_roundtrip(host, req.as_bytes(), connect_timeout, read_timeout)
}

pub fn https_post(host: &str, path: &str, body: &str, connect_timeout: u64, read_timeout: u64) -> Result<String, String> {
    let req = format!(
        "POST {} HTTP/1.0\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        path, host, body.len(), body
    );
    let bytes = tls_roundtrip(host, req.as_bytes(), connect_timeout, read_timeout)?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}
