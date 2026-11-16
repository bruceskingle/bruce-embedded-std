//! An HTTP or HTTPS server with ESP-IDF's EspHttpServer's shape: made from a Configuration, with handlers added with
//! fn_handler, each given an embedded-svc Request of a SimHttpConnection, the simulator's EspHttpConnection. As
//! ESP-IDF's, it handles one request at a time, on a thread of its own, and a handler's path is matched without the
//! query, a `*` at its end matching any rest of it. An HTTP and an HTTPS server may share a port, see Port.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Cursor, Read, Write};
use std::marker::PhantomData;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use embedded_svc::http::server::{Connection, Request};
use embedded_svc::http::{Headers, Method, Query};
use embedded_svc::io::{ErrorType, Read as SvcRead, Write as SvcWrite};

use super::{SimError, X509, method_named};

/// The ports a server is on unless its Configuration says otherwise, which a simulated device sets to its own, see
/// set_default_ports, as several may run on one computer, where 80 and 443 are not free.
static DEFAULT_PORTS: Mutex<(u16, u16)> = Mutex::new((80, 443));

/// Sets the ports, HTTP and HTTPS, of servers made with the Configuration's defaults, see Configuration::default.
pub fn set_default_ports(http: u16, https: u16) {
    *DEFAULT_PORTS.lock().unwrap() = (http, https);
}

/// How the server is set up, with ESP-IDF's names, of which the simulator uses the ports and the certificate: with a
/// certificate and key it serves HTTPS, on https_port, otherwise HTTP, on http_port.
#[derive(Clone, Debug)]
pub struct Configuration {
    pub http_port: u16,
    pub https_port: u16,
    pub ctrl_port: u16,
    pub max_open_sockets: usize,
    pub max_uri_handlers: usize,
    pub max_resp_headers: usize,
    pub stack_size: usize,
    pub lru_purge_enable: bool,
    pub uri_match_wildcard: bool,
    pub session_timeout: Duration,
    pub server_certificate: Option<X509>,
    pub private_key: Option<X509>,
}

impl Default for Configuration {
    fn default() -> Self {
        let (http_port, https_port) = *DEFAULT_PORTS.lock().unwrap();
        Configuration {
            http_port,
            https_port,
            ctrl_port: 32768,
            max_open_sockets: 4,
            max_uri_handlers: 32,
            max_resp_headers: 8,
            stack_size: 6144,
            lru_purge_enable: false,
            uri_match_wildcard: false,
            session_timeout: Duration::from_secs(20 * 60),
            server_certificate: None,
            private_key: None,
        }
    }
}

/// A handler, given the connection of a request.
type Handler = Box<dyn for<'r> Fn(&mut SimHttpConnection<'r>) -> Result<(), String> + Send>;

/// The handlers of a server.
type Handlers = Arc<Mutex<Vec<(String, Method, Handler)>>>;

/// A port, and the servers on it: an HTTP one, an HTTPS one, or both, as a simulated device has, so that one address,
/// host and port, reaches both, as a real device's address does on ports 80 and 443. Which a connection is for is told
/// by its first byte, which is 22, a TLS handshake, for HTTPS, and a letter, the method's, for HTTP.
#[derive(Default)]
struct Port {
    plain: Option<Handlers>,
    tls: Option<(Arc<rustls::ServerConfig>, Handlers)>,
}

/// The ports the servers of this program are on.
static PORTS: Mutex<BTreeMap<u16, Arc<Mutex<Port>>>> = Mutex::new(BTreeMap::new());

/// The server, which stops when it is dropped, with a lifetime, as ESP-IDF's EspHttpServer has, which code names.
pub struct SimHttpServer<'a> {
    handlers: Handlers,
    port: u16,
    tls: bool,
    _lifetime: PhantomData<&'a ()>,
}

impl SimHttpServer<'_> {
    pub fn new(configuration: &Configuration) -> Result<Self, SimError> {
        let tls = match (&configuration.server_certificate, &configuration.private_key) {
            (Some(certificate), Some(key)) => Some(tls_config(certificate.data(), key.data())?),
            _ => None,
        };
        let port = if tls.is_some() {
            configuration.https_port
        } else {
            configuration.http_port
        };
        let handlers: Handlers = Arc::new(Mutex::new(Vec::new()));
        let mut ports = PORTS.lock().unwrap();
        let shared = match ports.get(&port).filter(|_| port != 0) {
            Some(shared) => shared.clone(),
            None => {
                let listener = bind(port)?;
                // The port it is on, which the system chose if port is 0.
                let port = listener.local_addr()?.port();
                let shared = Arc::new(Mutex::new(Port::default()));
                ports.insert(port, shared.clone());
                let listening = shared.clone();
                std::thread::Builder::new()
                    .name(format!("http-{port}"))
                    .spawn(move || listen(listener, port, listening))
                    .map_err(SimError)?;
                shared
            }
        };
        let port = *ports
            .iter()
            .find(|(_, other)| Arc::ptr_eq(other, &shared))
            .map(|(port, _)| port)
            .unwrap_or(&port);
        let mut sharing = shared.lock().unwrap();
        let is_tls = tls.is_some();
        match tls {
            Some(config) if sharing.tls.is_none() => sharing.tls = Some((config, handlers.clone())),
            None if sharing.plain.is_none() => sharing.plain = Some(handlers.clone()),
            _ => {
                return Err(SimError::new(format!(
                    "There is an {} server on port {port} already",
                    if is_tls { "HTTPS" } else { "HTTP" }
                )));
            }
        }
        Ok(SimHttpServer {
            handlers,
            port,
            tls: is_tls,
            _lifetime: PhantomData,
        })
    }

    /// The port it is on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Adds a handler of requests with the method to uri, see the module's doc for how uri is matched.
    pub fn fn_handler<E, F>(&mut self, uri: &str, method: Method, f: F) -> Result<&mut Self, SimError>
    where
        F: for<'r> Fn(Request<&mut SimHttpConnection<'r>>) -> Result<(), E> + Send + 'static,
        E: std::fmt::Debug,
    {
        let handler: Handler =
            Box::new(move |connection| f(Request::wrap(connection)).map_err(|error| format!("{error:?}")));
        self.handlers.lock().unwrap().push((uri.to_string(), method, handler));
        Ok(self)
    }
}

impl Drop for SimHttpServer<'_> {
    /// Takes the server off its port, and stops listening on it once no server is on it.
    fn drop(&mut self) {
        let mut ports = PORTS.lock().unwrap();
        let Some(shared) = ports.get(&self.port).cloned() else {
            return;
        };
        let mut sharing = shared.lock().unwrap();
        if self.tls {
            sharing.tls = None;
        } else {
            sharing.plain = None;
        }
        if sharing.tls.is_none() && sharing.plain.is_none() {
            ports.remove(&self.port);
            drop(sharing);
            // Wakes the thread from accept, so that it sees it is to stop.
            let _ = TcpStream::connect(("127.0.0.1", self.port));
        }
    }
}

/// Listens on the port, waiting a few seconds if it is in use, as it is while the program which had it, e.g. the same
/// device before it restarted itself, see esp_restart, is ending.
fn bind(port: u16) -> Result<TcpListener, SimError> {
    let mut tries = 0;
    loop {
        match TcpListener::bind(("0.0.0.0", port)) {
            Ok(listener) => return Ok(listener),
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse && tries < 50 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(SimError::new(format!("Unable to listen on port {port}, {error}"))),
        }
    }
}

/// Serves the connections to the port, one at a time, each to its server, see Port, until the port has no servers.
fn listen(listener: TcpListener, port: u16, shared: Arc<Mutex<Port>>) {
    for stream in listener.incoming() {
        if !PORTS.lock().unwrap().contains_key(&port) {
            break;
        }
        let Ok(stream) = stream else { continue };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
        let mut first = [0u8; 1];
        let is_tls = matches!(stream.peek(&mut first), Ok(1)) && first[0] == 22;
        let (plain, tls) = {
            let sharing = shared.lock().unwrap();
            (sharing.plain.clone(), sharing.tls.clone())
        };
        let result = match (is_tls, tls, plain) {
            (true, Some((config, handlers)), _) => rustls::ServerConnection::new(config)
                .map_err(|error| SimError::new(format!("{error}")))
                .and_then(|connection| {
                    let mut stream = rustls::StreamOwned::new(connection, stream);
                    let served = serve(&mut stream, &handlers);
                    // The response's body ends when the connection closes, so TLS's close_notify says that it ended
                    // there, rather than being cut short, which a client, e.g. rustls, otherwise takes it to be.
                    stream.conn.send_close_notify();
                    let _ = stream.flush();
                    served
                }),
            (false, _, Some(handlers)) => serve(stream, &handlers),
            _ => Err(SimError::new(format!(
                "No {} server on port {port}",
                if is_tls { "HTTPS" } else { "HTTP" }
            ))),
        };
        if let Err(error) = result {
            log::debug!("A request on port {port} failed, {error}");
        }
    }
}

/// Whether the handler's uri matches the request's path.
fn matches(pattern: &str, path: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => path.starts_with(prefix),
        None => pattern == path,
    }
}

/// The largest request head and body read, to protect the computer from a client which never stops.
const MAX_HEAD: usize = 16 * 1024;
const MAX_BODY: usize = 16 * 1024 * 1024;

/// Reads one request from the stream, gives it to its handler, and closes the connection.
fn serve<S: Read + Write>(mut stream: S, handlers: &Handlers) -> Result<(), SimError> {
    let head = {
        let mut reader = BufReader::new(&mut stream);
        let head = read_head(&mut reader)?;
        let length: usize = head
            .header("Content-Length")
            .and_then(|length| length.trim().parse().ok())
            .unwrap_or(0);
        if length > MAX_BODY {
            return Err(SimError::new("The request's body is too large"));
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body)?;
        (head, body)
    };
    let (head, body) = head;
    let path = head.uri.split('?').next().unwrap_or_default().to_string();
    let handlers = handlers.lock().unwrap();
    let handler = handlers
        .iter()
        .find(|(uri, method, _)| *method == head.method && matches(uri, &path))
        .map(|(_, _, handler)| handler);
    let mut writer = std::io::BufWriter::new(&mut stream);
    let mut connection = SimHttpConnection {
        head,
        body: Body(Cursor::new(body)),
        out: &mut writer,
        response_initiated: false,
        _lifetime: PhantomData,
    };
    match handler {
        Some(handler) => {
            if let Err(error) = handler(&mut connection) {
                log::warn!(
                    "{} {} failed, {error}",
                    super::method_name(connection.head.method),
                    connection.head.uri
                );
                if !connection.response_initiated {
                    connection.initiate_response(500, Some("Internal Server Error"), &[])?;
                    connection.write_all_bytes(error.as_bytes())?;
                }
            }
        }
        None => {
            connection.initiate_response(404, Some("Not Found"), &[])?;
        }
    }
    if !connection.response_initiated {
        connection.initiate_response(200, Some("OK"), &[])?;
    }
    drop(connection);
    writer.flush()?;
    Ok(())
}

/// The request line and headers.
#[derive(Debug, Clone)]
pub struct RequestHead {
    method: Method,
    uri: String,
    headers: Vec<(String, String)>,
}

fn read_head(reader: &mut impl BufRead) -> Result<RequestHead, SimError> {
    let mut read = 0;
    let mut line = String::new();
    let mut next_line = |line: &mut String| -> Result<(), SimError> {
        line.clear();
        read += reader.read_line(line)?;
        if read > MAX_HEAD {
            return Err(SimError::new("The request's head is too large"));
        }
        Ok(())
    };
    next_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts
        .next()
        .and_then(method_named)
        .ok_or_else(|| SimError::new(format!("Not an HTTP request: {}", line.trim())))?;
    let uri = parts.next().unwrap_or("/").to_string();
    let mut headers = Vec::new();
    loop {
        next_line(&mut line)?;
        let header = line.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    Ok(RequestHead { method, uri, headers })
}

impl Query for RequestHead {
    fn uri(&self) -> &'_ str {
        &self.uri
    }

    fn method(&self) -> Method {
        self.method
    }
}

impl Headers for RequestHead {
    fn header(&self, name: &str) -> Option<&'_ str> {
        self.headers
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// A request's body, read before its handler is called.
pub struct Body(Cursor<Vec<u8>>);

impl ErrorType for Body {
    type Error = SimError;
}

impl SvcRead for Body {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        Ok(Read::read(&mut self.0, buf)?)
    }
}

/// The connection of a request, as ESP-IDF's EspHttpConnection, which its handler reads the request from and writes the
/// response to, see embedded-svc's Request and Response.
pub struct SimHttpConnection<'a> {
    head: RequestHead,
    body: Body,
    out: &'a mut dyn Write,
    response_initiated: bool,
    _lifetime: PhantomData<&'a ()>,
}

impl SimHttpConnection<'_> {
    fn write_all_bytes(&mut self, bytes: &[u8]) -> Result<(), SimError> {
        Ok(self.out.write_all(bytes)?)
    }
}

impl ErrorType for SimHttpConnection<'_> {
    type Error = SimError;
}

impl SvcRead for SimHttpConnection<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        Ok(Read::read(&mut self.body.0, buf)?)
    }
}

impl SvcWrite for SimHttpConnection<'_> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if !self.response_initiated {
            return Err(SimError::new("The response has not been started"));
        }
        Ok(self.out.write(buf)?)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(self.out.flush()?)
    }
}

impl Query for SimHttpConnection<'_> {
    fn uri(&self) -> &'_ str {
        self.head.uri()
    }

    fn method(&self) -> Method {
        self.head.method
    }
}

impl Headers for SimHttpConnection<'_> {
    fn header(&self, name: &str) -> Option<&'_ str> {
        self.head.header(name)
    }
}

impl<'a> Connection for SimHttpConnection<'a> {
    type Headers = RequestHead;
    type Read = Body;
    type RawConnectionError = SimError;
    type RawConnection = super::NoRawConnection;

    fn split(&mut self) -> (&Self::Headers, &mut Self::Read) {
        (&self.head, &mut self.body)
    }

    fn initiate_response<'b>(
        &'b mut self,
        status: u16,
        message: Option<&'b str>,
        headers: &'b [(&'b str, &'b str)],
    ) -> Result<(), Self::Error> {
        if self.response_initiated {
            return Err(SimError::new("The response has already been started"));
        }
        self.response_initiated = true;
        let mut head = format!("HTTP/1.1 {status} {}\r\n", message.unwrap_or(reason(status)));
        for (name, value) in headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        // The body ends when the connection closes, as one request is handled on each.
        head.push_str("Connection: close\r\n\r\n");
        self.write_all_bytes(head.as_bytes())
    }

    fn is_response_initiated(&self) -> bool {
        self.response_initiated
    }

    fn raw_connection(&mut self) -> Result<&mut Self::RawConnection, Self::Error> {
        Err(SimError::new("The simulator's server has no raw connection"))
    }
}

/// The usual reason phrase of a status.
fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "",
    }
}

/// The TLS set up of a server with the certificate and key, as PEM.
fn tls_config(certificate: &[u8], key: &[u8]) -> Result<Arc<rustls::ServerConfig>, SimError> {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let certificates = CertificateDer::pem_slice_iter(certificate)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| SimError::new(format!("The server's certificate is not PEM, {error}")))?;
    let key = PrivateKeyDer::from_pem_slice(key)
        .map_err(|error| SimError::new(format!("The server's key is not PEM, {error}")))?;
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|error| SimError::new(format!("{error}")))?
        .with_no_client_auth()
        .with_single_cert(certificates, key)
        .map_err(|error| SimError::new(format!("The server's certificate cannot be used, {error}")))?;
    Ok(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_match_exactly_or_by_prefix() {
        assert!(matches("/api/info", "/api/info"));
        assert!(!matches("/api/info", "/api/info/x"));
        assert!(matches("/api/store/*", "/api/store/changes"));
    }

    #[test]
    fn a_handler_reads_the_request_and_writes_the_response() {
        // Port 0, so that the system chooses a free one.
        let mut server = SimHttpServer::new(&Configuration {
            http_port: 0,
            ..Default::default()
        })
        .unwrap();
        let port = server.port();
        server
            .fn_handler("/echo", Method::Post, |mut request| {
                let mut body = vec![0; 5];
                let read = request.read(&mut body)?;
                let query = request.uri().to_string();
                let mut response = request.into_response(201, None, &[("Content-Type", "text/plain")])?;
                response.write(format!("{query} {}", String::from_utf8_lossy(&body[..read])).as_bytes())?;
                Ok::<(), SimError>(())
            })
            .unwrap();
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .write_all(b"POST /echo?x=1 HTTP/1.1\r\nHost: test\r\nContent-Length: 5\r\n\r\nhello")
            .unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        assert!(reply.starts_with("HTTP/1.1 201 Created\r\n"), "{reply}");
        assert!(reply.ends_with("\r\n\r\n/echo?x=1 hello"), "{reply}");

        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.write_all(b"GET /missing HTTP/1.1\r\n\r\n").unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        assert!(reply.starts_with("HTTP/1.1 404"), "{reply}");
    }
}
