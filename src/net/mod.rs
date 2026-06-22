pub mod msg;
use std::io::{self, Read, Write};
use std::net::{TcpStream, SocketAddr};
use std::sync::{Arc, Mutex, OnceLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

static DNS_CACHE: OnceLock<Option<SocketAddr>> = OnceLock::new();

/// Spawn a background thread at launch to resolve the game server hostname.
/// Call once at startup; result is cached in DNS_CACHE for `cached_server_addr`.
pub fn start_dns_prefetch() {
    thread::spawn(|| {
        use std::net::ToSocketAddrs;
        let resolved = "crumbonium.duckdns.org:7777"
            .to_socket_addrs().ok()
            .and_then(|mut it| it.next());
        let _ = DNS_CACHE.set(resolved);
    });
}

/// Return the pre-resolved server address if DNS has completed.
pub fn cached_server_addr() -> Option<SocketAddr> {
    DNS_CACHE.get().copied().flatten()
}

/// Encode a message as a length-prefixed bincode frame.
pub fn encode<T: serde::Serialize>(msg: &T) -> Option<Vec<u8>> {
    let payload = bincode::serialize(msg).ok()?;
    if payload.len() > 65536 { return None; }
    let mut buf = Vec::with_capacity(4 + payload.len());
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(&payload);
    Some(buf)
}

type ClientTlsStream = rustls::StreamOwned<rustls::ClientConnection, TcpStream>;

enum Inner {
    Plain(TcpStream),
    Tls(ClientTlsStream),
}

impl Inner {
    fn set_read_timeout(&self, dur: Option<Duration>) -> io::Result<()> {
        match self {
            Inner::Plain(s) => s.set_read_timeout(dur),
            Inner::Tls(s)   => s.get_ref().set_read_timeout(dur),
        }
    }
}

impl Read for Inner {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self { Inner::Plain(s) => s.read(buf), Inner::Tls(s) => s.read(buf) }
    }
}
impl Write for Inner {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self { Inner::Plain(s) => s.write(buf), Inner::Tls(s) => s.write(buf) }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self { Inner::Plain(s) => s.flush(), Inner::Tls(s) => s.flush() }
    }
}

pub struct ServerConn {
    stream:       Arc<Mutex<Inner>>,
    latest:       Arc<Mutex<Option<Vec<u8>>>>,
    welcome:      Arc<Mutex<Option<Vec<u8>>>>,
    disconnected: Arc<AtomicBool>,
}

fn make_tls_config() -> Arc<rustls::ClientConfig> {
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    Arc::new(rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth())
}

impl ServerConn {
    /// Connect using a pre-resolved SocketAddr, overriding the port with `port`.
    pub fn connect_addr(mut sock: SocketAddr, port: u16) -> io::Result<Self> {
        sock.set_port(port);
        let tcp = TcpStream::connect_timeout(&sock, Duration::from_secs(4))?;
        tcp.set_nodelay(true)?;
        let host = "crumbonium.duckdns.org";
        let inner = match rustls::pki_types::ServerName::try_from(host.to_string()) {
            Ok(name) => {
                let config = make_tls_config();
                match rustls::ClientConnection::new(config, name) {
                    Ok(conn) => Inner::Tls(rustls::StreamOwned::new(conn, tcp)),
                    Err(_)   => return Err(io::Error::new(io::ErrorKind::Other, "TLS init failed")),
                }
            }
            Err(_) => Inner::Plain(tcp),
        };
        inner.set_read_timeout(Some(Duration::from_secs(10)))?;
        let stream = Arc::new(Mutex::new(inner));
        Ok(Self { stream, latest: Arc::new(Mutex::new(None)), welcome: Arc::new(Mutex::new(None)), disconnected: Arc::new(AtomicBool::new(false)) })
    }

    pub fn connect(addr: &str) -> io::Result<Self> {
        use std::net::ToSocketAddrs;
        let sock = addr.to_socket_addrs()?.next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no addr"))?;
        let tcp = TcpStream::connect_timeout(&sock, Duration::from_secs(4))?;
        tcp.set_nodelay(true)?;

        let host = addr.split(':').next().unwrap_or(addr);
        let inner = match rustls::pki_types::ServerName::try_from(host.to_string()) {
            Ok(name) => {
                let config = make_tls_config();
                match rustls::ClientConnection::new(config, name) {
                    Ok(conn) => Inner::Tls(rustls::StreamOwned::new(conn, tcp)),
                    Err(_)   => return Err(io::Error::new(io::ErrorKind::Other, "TLS init failed")),
                }
            }
            Err(_) => Inner::Plain(tcp), // IP address — no TLS (local dev)
        };

        // 10s read timeout to detect half-open connections
        inner.set_read_timeout(Some(Duration::from_secs(10)))?;

        let stream = Arc::new(Mutex::new(inner));
        Ok(Self {
            stream,
            latest:       Arc::new(Mutex::new(None)),
            welcome:      Arc::new(Mutex::new(None)),
            disconnected: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn start_reader(&mut self) {
        let stream2       = self.stream.clone();
        let latest2       = self.latest.clone();
        let welcome2      = self.welcome.clone();
        let disconnected2 = self.disconnected.clone();
        thread::spawn(move || {
            let mut read_buf: Vec<u8> = Vec::new();
            loop {
                match read_one_arc(&stream2, &mut read_buf) {
                    Some(buf) => {
                        if bincode::deserialize::<crate::net::msg::WelcomeMsg>(&buf).is_ok() {
                            *welcome2.lock().unwrap() = Some(buf);
                        } else {
                            *latest2.lock().unwrap() = Some(buf);
                        }
                        // Yield so the main thread can acquire the stream lock
                        // to send InputMsg before we re-lock for the next read.
                        thread::sleep(Duration::from_millis(1));
                    }
                    None => break,
                }
            }
            disconnected2.store(true, Ordering::Relaxed);
        });
    }

    pub fn is_disconnected(&self) -> bool {
        self.disconnected.load(Ordering::Relaxed)
    }

    pub fn send<T: serde::Serialize>(&mut self, msg: &T) {
        if let Some(bytes) = encode(msg) {
            let mut guard = self.stream.lock().unwrap();
            if guard.write_all(&bytes).is_err() {
                self.disconnected.store(true, Ordering::Relaxed);
            }
        }
    }

    pub fn send_raw(&mut self, bytes: &[u8]) {
        let mut guard = self.stream.lock().unwrap();
        if guard.write_all(bytes).is_err() {
            self.disconnected.store(true, Ordering::Relaxed);
        }
    }

    /// Timeout-respecting read for lobby drain loops — returns None when no
    /// complete message arrives before the stream's current read timeout fires.
    /// Do NOT use after start_reader() (background thread owns the stream).
    pub fn recv_blocking<T: serde::de::DeserializeOwned>(&mut self) -> Option<T> {
        let mut read_buf = Vec::new();
        let buf = read_one_timeout(&self.stream, &mut read_buf)?;
        bincode::deserialize(&buf).ok()
    }

    pub fn try_recv_welcome(&mut self) -> Option<msg::WelcomeMsg> {
        let buf = self.welcome.lock().unwrap().take()?;
        bincode::deserialize(&buf).ok()
    }

    pub fn try_recv<T: serde::de::DeserializeOwned>(&mut self) -> Option<T> {
        let buf = self.latest.lock().unwrap().take()?;
        bincode::deserialize(&buf).ok()
    }

    pub fn set_read_timeout(&self, dur: Option<Duration>) {
        self.stream.lock().unwrap().set_read_timeout(dur).ok();
    }

    /// Read a line (up to \n) from the stream — used during handshake only.
    pub fn read_line_blocking(&mut self) -> String {
        use std::io::BufRead;
        let mut guard = self.stream.lock().unwrap();
        let mut line = String::new();
        // Read byte-by-byte to avoid consuming buffered data past the newline.
        loop {
            let mut b = [0u8; 1];
            match guard.read(&mut b) {
                Ok(1) => {
                    if b[0] == b'\n' { break; }
                    line.push(b[0] as char);
                }
                _ => break,
            }
        }
        line.trim().to_string()
    }

    pub fn buf_len(&self) -> usize { 0 }
}

/// Read one complete length-prefixed frame from a shared stream.
/// Holds the lock only briefly per chunk; releases between attempts so the
/// write path is never blocked more than ~5ms.
/// Read one complete frame, looping through WouldBlock/TimedOut (used by the
/// background start_reader() thread which must never give up on a live connection).
fn read_one_arc<S: Read>(stream: &Arc<Mutex<S>>, read_buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    loop {
        if read_buf.len() >= 4 {
            let len = u32::from_le_bytes(read_buf[..4].try_into().unwrap()) as usize;
            if len == 0 || len > 65536 { return None; }
            if read_buf.len() >= 4 + len {
                let frame = read_buf[4..4+len].to_vec();
                read_buf.drain(..4+len);
                return Some(frame);
            }
        }
        let mut tmp = [0u8; 4096];
        let n = {
            let mut guard = stream.lock().unwrap();
            match guard.read(&mut tmp) {
                Ok(0) => return None,
                Ok(n) => n,
                Err(e) if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) => {
                    drop(guard);
                    thread::sleep(Duration::from_millis(1));
                    continue;
                }
                Err(_) => return None,
            }
        };
        read_buf.extend_from_slice(&tmp[..n]);
    }
}

/// Like read_one_arc but returns None on WouldBlock/TimedOut instead of looping.
/// Used by recv_blocking() in the lobby so the caller can draw frames and poll input.
fn read_one_timeout<S: Read>(stream: &Arc<Mutex<S>>, read_buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    loop {
        if read_buf.len() >= 4 {
            let len = u32::from_le_bytes(read_buf[..4].try_into().unwrap()) as usize;
            if len == 0 || len > 65536 { return None; }
            if read_buf.len() >= 4 + len {
                let frame = read_buf[4..4+len].to_vec();
                read_buf.drain(..4+len);
                return Some(frame);
            }
        }
        let mut tmp = [0u8; 4096];
        let n = {
            let mut guard = stream.lock().unwrap();
            match guard.read(&mut tmp) {
                Ok(0) => return None,
                Ok(n) => n,
                Err(e) if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) => {
                    return None; // caller will retry with a fresh call
                }
                Err(_) => return None,
            }
        };
        read_buf.extend_from_slice(&tmp[..n]);
    }
}
