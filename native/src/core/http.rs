//! A small blocking HTTP/1.1 client for one Syncthing GUI endpoint, with the
//! bounds of Syncshell's Go `http.Transport`: 3 s to connect, 4 s for the
//! TLS handshake, 8 s from the request to the response headers, an overall
//! per-request deadline, at most two idle keep-alive connections (dropped
//! after 45 s), no proxy, no compression, and a cap on the body. Redirects
//! are the caller's business (see `client.rs`).
//!
//! The bounds are absolute, like Go's timers: every socket read and write,
//! including rustls's own, goes through `foamy_helper::net::DeadlineStream`,
//! whose deadline is the phase's (handshake, headers, or the whole request),
//! so a peer that trickles bytes can't stretch a phase past its bound.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use foamy_helper::net::{DeadlineStream, TimeoutStream, is_deadline};
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, StreamOwned};

const DIAL_TIMEOUT: Duration = Duration::from_secs(3);
const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(4);
const RESPONSE_HEADER_TIMEOUT: Duration = Duration::from_secs(8);
const IDLE_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_IDLE: usize = 2;
/// Non-GET requests reuse only a connection this fresh, so a mutation never
/// races a server closing an idle connection.
const FRESH_FOR_MUTATIONS: Duration = Duration::from_secs(2);
const MAX_HEADER_BYTES: usize = 1 << 20;

/// Where the endpoint is.
#[derive(Clone, Debug)]
pub enum Address {
    /// `host:port` as dialled (an IPv6 host without brackets).
    Tcp {
        host: String,
        port: u16,
    },
    Unix(String),
}

/// Why a request failed, as Syncshell classified it.
#[derive(Debug)]
pub enum Failure {
    /// Handshake or certificate verification (Go: an error mentioning TLS).
    Tls,
    /// This CPU can't run the TLS cryptography (checked before connecting).
    TlsUnsupported,
    /// The deadline passed (dial, headers or body).
    Timeout,
    /// The host name did not resolve.
    Dns,
    /// Anything else on the way (refused, reset, malformed reply).
    Connection,
}

pub struct Response {
    pub status: u16,
    /// `Location`, for redirects.
    pub location: Option<String>,
    /// The body, at most `max_body` bytes; `None` when it was longer.
    pub body: Option<Vec<u8>>,
}

pub struct Transport {
    address: Address,
    host_header: String,
    tls: Option<(Arc<ClientConfig>, ServerName<'static>)>,
    /// Checked before any handshake (Graviola needs these instructions).
    tls_usable: bool,
    idle: Mutex<Vec<(Instant, Connection)>>,
}

enum Socket {
    Tcp(TcpStream),
    Unix(UnixStream),
}

impl TimeoutStream for Socket {
    fn set_read_limit(&self, timeout: Option<Duration>) -> io::Result<()> {
        match self {
            Socket::Tcp(s) => s.set_read_timeout(timeout),
            Socket::Unix(s) => s.set_read_timeout(timeout),
        }
    }
    fn set_write_limit(&self, timeout: Option<Duration>) -> io::Result<()> {
        match self {
            Socket::Tcp(s) => s.set_write_timeout(timeout),
            Socket::Unix(s) => s.set_write_timeout(timeout),
        }
    }
}

/// A socket whose every read and write ends by the current phase's deadline.
type Timed = DeadlineStream<Socket>;

impl Socket {
    /// True when the peer closed or sent something while the connection
    /// sat idle (either way it can't carry the next request).
    fn stale(&self) -> bool {
        use std::os::fd::AsFd;
        let fd = match self {
            Socket::Tcp(s) => s.as_fd(),
            Socket::Unix(s) => s.as_fd(),
        };
        let mut fds = [rustix::event::PollFd::new(
            &fd,
            rustix::event::PollFlags::IN,
        )];
        let zero = rustix::event::Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        !matches!(rustix::event::poll(&mut fds, Some(&zero)), Ok(0))
    }

    fn shutdown(&self) {
        let _ = match self {
            Socket::Tcp(s) => s.shutdown(Shutdown::Both),
            Socket::Unix(s) => s.shutdown(Shutdown::Both),
        };
    }
}

impl Read for Socket {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Socket::Tcp(s) => s.read(buf),
            Socket::Unix(s) => s.read(buf),
        }
    }
}

impl Write for Socket {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Socket::Tcp(s) => s.write(buf),
            Socket::Unix(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Socket::Tcp(s) => s.flush(),
            Socket::Unix(s) => s.flush(),
        }
    }
}

enum Stream {
    Plain(Timed),
    Tls(Box<StreamOwned<ClientConnection, Timed>>),
}

impl Stream {
    fn socket(&self) -> &Socket {
        match self {
            Stream::Plain(s) => s.get_ref(),
            Stream::Tls(s) => s.sock.get_ref(),
        }
    }

    /// Bounds all I/O from now on (TLS records included) by `deadline`.
    fn set_deadline(&mut self, deadline: Instant) {
        match self {
            Stream::Plain(s) => s.set_deadline(deadline),
            Stream::Tls(s) => s.sock.set_deadline(deadline),
        }
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Stream::Plain(s) => s.read(buf),
            Stream::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Stream::Plain(s) => s.write(buf),
            Stream::Tls(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Stream::Plain(s) => s.flush(),
            Stream::Tls(s) => s.flush(),
        }
    }
}

struct Connection {
    reader: BufReader<Stream>,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.reader.get_ref().socket().shutdown();
    }
}

fn io_failure(error: &io::Error) -> Failure {
    if is_deadline(error) {
        return Failure::Timeout;
    }
    // A TLS alert or record error surfaces from rustls as InvalidData.
    if error
        .get_ref()
        .is_some_and(|inner| inner.is::<rustls::Error>())
    {
        return Failure::Tls;
    }
    Failure::Connection
}

impl Transport {
    pub fn new(address: Address, tls: Option<Arc<ClientConfig>>) -> Transport {
        let (host_header, name_host) = match &address {
            Address::Tcp { host, port } if host.contains(':') => {
                (format!("[{host}]:{port}"), host.clone())
            }
            Address::Tcp { host, port } => (format!("{host}:{port}"), host.clone()),
            Address::Unix(_) => ("unix".to_string(), "unix".to_string()),
        };
        let tls = tls.map(|config| (config, crate::core::tls::server_name(&name_host)));
        Transport {
            address,
            host_header,
            tls_usable: tls.is_none() || foamy_helper::tls::cpu_supported(),
            tls,
            idle: Mutex::new(Vec::new()),
        }
    }

    /// The `Host` header value (Go's `URL.Host`), for same-origin checks.
    pub fn host(&self) -> &str {
        &self.host_header
    }

    fn dial(&self, deadline: Instant) -> Result<Connection, Failure> {
        let remaining = |limit: Duration| -> Result<Duration, Failure> {
            let left = deadline
                .saturating_duration_since(Instant::now())
                .min(limit);
            if left.is_zero() {
                Err(Failure::Timeout)
            } else {
                Ok(left)
            }
        };
        let dial_deadline = Instant::now() + remaining(DIAL_TIMEOUT)?;
        let socket = match &self.address {
            Address::Tcp { host, port } => {
                let addresses: Vec<_> = (host.as_str(), *port)
                    .to_socket_addrs()
                    .map_err(|_| Failure::Dns)?
                    .collect();
                if addresses.is_empty() {
                    return Err(Failure::Dns);
                }
                let mut last = Failure::Connection;
                let mut connected = None;
                for (index, address) in addresses.iter().enumerate() {
                    // Like Go's dialSerial: each address gets its share of what is left.
                    let left = dial_deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        last = Failure::Timeout;
                        break;
                    }
                    let share = left / (addresses.len() - index) as u32;
                    match TcpStream::connect_timeout(address, share.max(Duration::from_millis(1))) {
                        Ok(stream) => {
                            connected = Some(stream);
                            break;
                        }
                        Err(error) => last = io_failure(&error),
                    }
                }
                let stream = connected.ok_or(last)?;
                let _ = stream.set_nodelay(true);
                Socket::Tcp(stream)
            }
            Address::Unix(path) => {
                Socket::Unix(UnixStream::connect(path).map_err(|e| io_failure(&e))?)
            }
        };
        let stream = match &self.tls {
            None => Stream::Plain(DeadlineStream::new(socket, deadline)),
            Some((config, name)) => {
                if !self.tls_usable {
                    return Err(Failure::TlsUnsupported);
                }
                let handshake_deadline = Instant::now() + remaining(TLS_HANDSHAKE_TIMEOUT)?;
                // Go: "net/http: TLS handshake timeout" counts as a TLS
                // failure; the request's own deadline passing first is
                // Client.Timeout, a timeout.
                let expired = if handshake_deadline < deadline {
                    Failure::Tls
                } else {
                    Failure::Timeout
                };
                let connection = ClientConnection::new(Arc::clone(config), name.clone())
                    .map_err(|_| Failure::Tls)?;
                let mut tls =
                    StreamOwned::new(connection, DeadlineStream::new(socket, handshake_deadline));
                while tls.conn.is_handshaking() {
                    match tls.conn.complete_io(&mut tls.sock) {
                        Ok(_) => {}
                        Err(error) if is_deadline(&error) => return Err(expired),
                        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                            return Err(Failure::Tls);
                        }
                        Err(_) => return Err(Failure::Connection),
                    }
                }
                Stream::Tls(Box::new(tls))
            }
        };
        Ok(Connection {
            reader: BufReader::with_capacity(16 * 1024, stream),
        })
    }

    fn take_idle(&self, method: &str) -> Option<Connection> {
        let mut idle = self.idle.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        while let Some((since, connection)) = idle.pop() {
            let age = now.saturating_duration_since(since);
            if age >= IDLE_TIMEOUT || connection.reader.get_ref().socket().stale() {
                continue;
            }
            if method != "GET" && age >= FRESH_FOR_MUTATIONS {
                continue;
            }
            return Some(connection);
        }
        None
    }

    fn put_idle(&self, connection: Connection) {
        let mut idle = self.idle.lock().unwrap_or_else(|e| e.into_inner());
        if idle.len() < MAX_IDLE {
            idle.push((Instant::now(), connection));
        }
    }

    /// One request; `headers` are sent as given (the caller adds the API key).
    pub fn request(
        &self,
        method: &str,
        target: &str,
        headers: &[(&str, &str)],
        body: Option<&[u8]>,
        timeout: Duration,
        max_body: usize,
    ) -> Result<Response, Failure> {
        let deadline = Instant::now() + timeout;
        let mut head = format!(
            "{method} {target} HTTP/1.1\r\nHost: {}\r\nUser-Agent: foamy-syncthing-helper\r\n",
            self.host_header
        );
        for (name, value) in headers {
            head.push_str(name);
            head.push_str(": ");
            head.push_str(value);
            head.push_str("\r\n");
        }
        if method != "GET" || body.is_some() {
            head.push_str(&format!(
                "Content-Length: {}\r\n",
                body.map_or(0, <[u8]>::len)
            ));
        }
        head.push_str("\r\n");
        let mut message = head.into_bytes();
        if let Some(body) = body {
            message.extend_from_slice(body);
        }
        // A reused connection the server already closed fails before any
        // response byte: a GET then retries once on a new connection.
        if let Some(connection) = self.take_idle(method) {
            match self.exchange(connection, &message, deadline, max_body) {
                Ok(response) => return Ok(response),
                Err((failure, answered)) => {
                    if answered || method != "GET" || matches!(failure, Failure::Timeout) {
                        return Err(failure);
                    }
                }
            }
        }
        let connection = self.dial(deadline)?;
        self.exchange(connection, &message, deadline, max_body)
            .map_err(|(failure, _)| failure)
    }

    /// Writes the request and reads the response; the bool is "the server
    /// started answering".
    fn exchange(
        &self,
        mut connection: Connection,
        message: &[u8],
        deadline: Instant,
        max_body: usize,
    ) -> Result<Response, (Failure, bool)> {
        if Instant::now() >= deadline {
            return Err((Failure::Timeout, false));
        }
        // Writing the request: the request's deadline (Go: Client.Timeout).
        connection.reader.get_mut().set_deadline(deadline);
        {
            let stream = connection.reader.get_mut();
            stream
                .write_all(message)
                .and_then(|()| stream.flush())
                .map_err(|e| (io_failure(&e), false))?;
        }
        // The headers, interim 1xx ones included: 8 s from the written
        // request (Go: ResponseHeaderTimeout), within the request's deadline.
        let header_deadline = (Instant::now() + RESPONSE_HEADER_TIMEOUT).min(deadline);
        connection.reader.get_mut().set_deadline(header_deadline);
        let (status, headers) = loop {
            let (status, headers) = read_head(&mut connection.reader)?;
            // Interim 1xx responses precede the real one.
            if (100..200).contains(&status) && status != 101 {
                continue;
            }
            break (status, headers);
        };
        // The body: what is left of the request's deadline.
        connection.reader.get_mut().set_deadline(deadline);
        let header = |name: &str| {
            headers
                .iter()
                .rev()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.as_str())
        };
        let location = header("Location").map(str::to_string);
        let mut reusable =
            !header("Connection").is_some_and(|v| v.to_ascii_lowercase().contains("close"));
        let chunked =
            header("Transfer-Encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
        let length = header("Content-Length").map(|v| v.trim().parse::<u64>());
        if !(200..300).contains(&status) {
            // Like Go, an error reply's body is not read; the connection goes.
            return Ok(Response {
                status,
                location,
                body: Some(Vec::new()),
            });
        }
        let mut body = Vec::new();
        let complete = if status == 204 || status == 304 {
            true
        } else if chunked {
            read_chunked(&mut connection.reader, &mut body, max_body)
                .map_err(|e| (io_failure(&e), true))?
        } else if let Some(length) = length {
            let length = length.map_err(|_| (Failure::Connection, true))?;
            let wanted = length.min(max_body as u64 + 1);
            (&mut connection.reader)
                .take(wanted)
                .read_to_end(&mut body)
                .map_err(|e| (io_failure(&e), true))?;
            if (body.len() as u64) < wanted {
                return Err((Failure::Connection, true));
            }
            body.len() as u64 == length
        } else {
            reusable = false;
            (&mut connection.reader)
                .take(max_body as u64 + 1)
                .read_to_end(&mut body)
                .map_err(|e| (io_failure(&e), true))?;
            true
        };
        if body.len() > max_body {
            return Ok(Response {
                status,
                location,
                body: None,
            });
        }
        if complete && reusable && connection.reader.buffer().is_empty() {
            self.put_idle(connection);
        }
        Ok(Response {
            status,
            location,
            body: Some(body),
        })
    }
}

/// The status line and headers.
type Head = (u16, Vec<(String, String)>);

fn read_head(reader: &mut BufReader<Stream>) -> Result<Head, (Failure, bool)> {
    let mut total = 0usize;
    let mut answered = false;
    let mut next_line = |reader: &mut BufReader<Stream>| -> Result<String, (Failure, bool)> {
        let mut line = Vec::new();
        let read = (&mut *reader)
            .take((MAX_HEADER_BYTES - total.min(MAX_HEADER_BYTES)) as u64 + 1)
            .read_until(b'\n', &mut line)
            .map_err(|e| (io_failure(&e), answered))?;
        if read == 0 {
            return Err((Failure::Connection, answered));
        }
        answered = true;
        total += read;
        if total > MAX_HEADER_BYTES || !line.ends_with(b"\n") {
            return Err((Failure::Connection, true));
        }
        while line.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
            line.pop();
        }
        String::from_utf8(line).map_err(|_| (Failure::Connection, true))
    };
    let status_line = next_line(reader)?;
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or("");
    let status = parts.next().and_then(|code| code.parse::<u16>().ok());
    let (true, Some(status)) = (version.starts_with("HTTP/1."), status) else {
        return Err((Failure::Connection, true));
    };
    let mut headers = Vec::new();
    loop {
        let line = next_line(reader)?;
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err((Failure::Connection, true));
        };
        headers.push((name.trim().to_string(), value.trim().to_string()));
    }
    if version == "HTTP/1.0"
        && !headers.iter().any(|(k, v)| {
            k.eq_ignore_ascii_case("Connection") && v.eq_ignore_ascii_case("keep-alive")
        })
    {
        headers.push(("Connection".into(), "close".into()));
    }
    Ok((status, headers))
}

/// Decodes a chunked body into `body` (at most `max + 1` bytes kept);
/// true when the terminating chunk and trailers were read.
fn read_chunked<R: BufRead>(reader: &mut R, body: &mut Vec<u8>, max: usize) -> io::Result<bool> {
    let invalid = || io::Error::new(io::ErrorKind::UnexpectedEof, "bad chunk");
    loop {
        let mut line = Vec::new();
        (&mut *reader).take(1024).read_until(b'\n', &mut line)?;
        if !line.ends_with(b"\n") {
            return Err(invalid());
        }
        let text = String::from_utf8_lossy(&line);
        let size = text.trim().split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size, 16).map_err(|_| invalid())?;
        if size == 0 {
            // Trailers until an empty line.
            loop {
                let mut trailer = Vec::new();
                (&mut *reader).take(8192).read_until(b'\n', &mut trailer)?;
                if !trailer.ends_with(b"\n") {
                    return Err(invalid());
                }
                if trailer == b"\r\n" || trailer == b"\n" {
                    return Ok(true);
                }
            }
        }
        // Over the cap: keep one byte more than allowed and stop (no
        // arithmetic on the peer's size can wrap).
        let room = (max + 1).saturating_sub(body.len());
        if size > room {
            (&mut *reader).take(room as u64).read_to_end(body)?;
            return Ok(false);
        }
        let before = body.len();
        (&mut *reader).take(size as u64).read_to_end(body)?;
        if body.len() - before != size {
            return Err(invalid());
        }
        let mut end = [0u8; 2];
        reader.read_exact(&mut end[..1])?;
        if end[0] == b'\r' {
            reader.read_exact(&mut end[1..])?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn chunked(input: &[u8], max: usize) -> (io::Result<bool>, Vec<u8>) {
        let mut body = Vec::new();
        let result = read_chunked(&mut Cursor::new(input.to_vec()), &mut body, max);
        (result, body)
    }

    /// A loopback peer that reads one request, sends `first`, then `piece`
    /// every 50 ms for as long as the client listens.
    fn trickler(first: &'static [u8], piece: &'static [u8]) -> Transport {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut peer, _) = listener.accept().unwrap();
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && peer.read(&mut byte).unwrap_or(0) == 1 {
                head.push(byte[0]);
            }
            let _ = peer.write_all(first);
            while peer.write_all(piece).is_ok() {
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        let address = Address::Tcp {
            host: "127.0.0.1".into(),
            port,
        };
        Transport::new(address, None)
    }

    #[test]
    fn a_trickling_peer_cannot_outlast_the_request_deadline() {
        for (first, piece) in [
            // Headers that never end.
            (&b"HTTP/1.1 200 OK\r\n"[..], &b"X-Trickle: 1\r\n"[..]),
            // A body that never completes.
            (
                &b"HTTP/1.1 200 OK\r\nContent-Length: 100000\r\n\r\n"[..],
                &b" "[..],
            ),
        ] {
            let transport = trickler(first, piece);
            let started = Instant::now();
            let result = transport.request(
                "GET",
                "/rest/noauth/health",
                &[],
                None,
                Duration::from_millis(700),
                1 << 20,
            );
            let took = started.elapsed();
            assert!(matches!(result, Err(Failure::Timeout)), "{first:?}");
            assert!(
                took >= Duration::from_millis(650) && took < Duration::from_millis(1500),
                "{took:?}"
            );
        }
    }

    #[test]
    fn chunked_bodies_decode_within_the_cap() {
        let (result, body) = chunked(b"3\r\nabc\r\n2;ext=1\r\nde\r\n0\r\nTrailer: x\r\n\r\n", 100);
        assert!(result.unwrap());
        assert_eq!(body, b"abcde");
        // Too much: one byte over the cap is kept, then it stops.
        let (result, body) = chunked(b"5\r\nabcde\r\n0\r\n\r\n", 3);
        assert!(!result.unwrap());
        assert_eq!(body, b"abcd");
        // A size near usize::MAX can't wrap the check or allocate.
        let (result, body) = chunked(b"ffffffffffffffff\r\nab", 3);
        assert!(!result.unwrap());
        assert_eq!(body, b"ab");
        for bad in [
            &b"zz\r\n"[..],
            b"3\r\nab",
            b"10000000000000000\r\n",
            b"3\r\nabc\r\n0\r\nno end",
        ] {
            assert!(
                chunked(bad, 100).0.is_err(),
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
    }
}
