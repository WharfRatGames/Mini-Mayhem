pub mod msg;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

/// Encode a message as a length-prefixed bincode frame.
/// Returns None only if bincode serialization fails (should never happen for our structs).
pub fn encode<T: serde::Serialize>(msg: &T) -> Option<Vec<u8>> {
    let payload = bincode::serialize(msg).ok()?;
    if payload.len() > 65536 { return None; }
    let mut buf = Vec::with_capacity(4 + payload.len());
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(&payload);
    Some(buf)
}

/// Read one length-prefixed frame.  Returns None on any IO error, timeout, or oversized packet.
fn read_one(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut hdr = [0u8; 4];
    stream.read_exact(&mut hdr).ok()?;
    let len = u32::from_le_bytes(hdr) as usize;
    if len == 0 || len > 65536 { return None; }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).ok()?;
    Some(buf)
}

pub struct ServerConn {
    pub stream:   TcpStream,
    latest:       Arc<Mutex<Option<Vec<u8>>>>,
    welcome:      Arc<Mutex<Option<Vec<u8>>>>,
    /// Set to true when the reader thread exits or a write fails.
    disconnected: Arc<AtomicBool>,
}

impl ServerConn {
    pub fn connect(addr: &str) -> std::io::Result<Self> {
        use std::net::ToSocketAddrs;
        let sock = addr.to_socket_addrs()?.next()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no addr"))?;
        let stream = TcpStream::connect_timeout(&sock, std::time::Duration::from_secs(4))?;
        stream.set_nodelay(true)?;
        // 10-second read timeout: detects half-open connections (server disappears without FIN).
        stream.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;
        let latest       = Arc::new(Mutex::new(None));
        let welcome      = Arc::new(Mutex::new(None));
        let disconnected = Arc::new(AtomicBool::new(false));
        Ok(Self { stream, latest, welcome, disconnected })
    }

    /// Spawn the background reader thread. Call once after the initial handshake.
    pub fn start_reader(&mut self) {
        let latest2       = self.latest.clone();
        let welcome2      = self.welcome.clone();
        let disconnected2 = self.disconnected.clone();
        let mut reader = match self.stream.try_clone() {
            Ok(r) => r,
            Err(_) => { disconnected2.store(true, Ordering::Relaxed); return; }
        };
        // The timeout is inherited from the original stream on clone (Linux, Windows).
        thread::spawn(move || {
            loop {
                match read_one(&mut reader) {
                    Some(buf) => {
                        if bincode::deserialize::<crate::net::msg::WelcomeMsg>(&buf).is_ok() {
                            *welcome2.lock().unwrap() = Some(buf);
                        } else {
                            *latest2.lock().unwrap() = Some(buf);
                        }
                    }
                    None => break, // IO error, timeout, or oversized packet → disconnect
                }
            }
            disconnected2.store(true, Ordering::Relaxed);
        });
    }

    /// Returns true if the connection has been lost (reader thread exited or write failed).
    pub fn is_disconnected(&self) -> bool {
        self.disconnected.load(Ordering::Relaxed)
    }

    /// Send a serializable message. Sets disconnected flag on write failure.
    pub fn send<T: serde::Serialize>(&mut self, msg: &T) {
        if let Some(bytes) = encode(msg) {
            if self.stream.write_all(&bytes).is_err() {
                self.disconnected.store(true, Ordering::Relaxed);
            }
        }
    }

    /// Send raw bytes. Sets disconnected flag on write failure.
    pub fn send_raw(&mut self, bytes: &[u8]) {
        if self.stream.write_all(bytes).is_err() {
            self.disconnected.store(true, Ordering::Relaxed);
        }
    }

    /// Blocking read of one message — use only during initial handshake before start_reader().
    pub fn recv_blocking<T: serde::de::DeserializeOwned>(&mut self) -> Option<T> {
        let buf = read_one(&mut self.stream)?;
        bincode::deserialize(&buf).ok()
    }

    /// Non-blocking: returns the latest WelcomeMsg if one arrived on the reader thread.
    pub fn try_recv_welcome(&mut self) -> Option<msg::WelcomeMsg> {
        let buf = self.welcome.lock().unwrap().take()?;
        bincode::deserialize(&buf).ok()
    }

    /// Non-blocking: returns the latest state message from the reader thread (lossy — newest wins).
    pub fn try_recv<T: serde::de::DeserializeOwned>(&mut self) -> Option<T> {
        let buf = self.latest.lock().unwrap().take()?;
        bincode::deserialize(&buf).ok()
    }

    pub fn buf_len(&self) -> usize { 0 }
}
