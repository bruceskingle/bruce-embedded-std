//! An HTTP server and client for the computer which have the same shape as ESP-IDF's, `esp_idf_svc::http`'s
//! EspHttpServer and EspHttpConnection, and implement embedded-svc's Connection traits, as they do, so that code
//! written for them, e.g. bruce-esp-idf's device API, its handlers' `Request<&mut EspHttpConnection>`, and a capture
//! device's polls, compiles for the computer as it is, given these under the same names, see docs/Simulator.md. It is
//! deliberately not a hardware-agnostic abstraction of HTTP, which bruce-embedded-std's http_server.rs explains does not
//! work: the same source is compiled against different types.

pub mod client;
pub mod nvs;
pub mod server;
pub mod sys;

pub use embedded_svc::http::Method;
pub use embedded_svc::io;

/// esp_idf_svc::http's names, for code written for ESP-IDF, see the module's doc.
pub mod http {
    pub use embedded_svc::http::{Headers, Method, Query, Status};

    pub mod server {
        pub use crate::svc::server::{
            Configuration, SimHttpConnection as EspHttpConnection, SimHttpServer as EspHttpServer,
        };
        pub use embedded_svc::http::server::{Request, Response};
    }

    pub mod client {
        pub use crate::svc::client::{Configuration, SimHttpConnection as EspHttpConnection};
    }
}

/// esp_idf_svc::tls's name of a certificate or key.
pub mod tls {
    pub use super::X509;
}

/// The error of a request, as ESP-IDF's EspIOError is, with the cause's text.
#[derive(Debug)]
pub struct SimError(pub std::io::Error);

impl SimError {
    pub fn new(message: impl Into<String>) -> Self {
        SimError(std::io::Error::other(message.into()))
    }
}

impl std::fmt::Display for SimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for SimError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

impl embedded_svc::io::Error for SimError {
    fn kind(&self) -> embedded_svc::io::ErrorKind {
        match self.0.kind() {
            std::io::ErrorKind::TimedOut => embedded_svc::io::ErrorKind::TimedOut,
            std::io::ErrorKind::InvalidInput => embedded_svc::io::ErrorKind::InvalidInput,
            _ => embedded_svc::io::ErrorKind::Other,
        }
    }
}

impl From<std::io::Error> for SimError {
    fn from(error: std::io::Error) -> Self {
        SimError(error)
    }
}

/// A certificate or key, as PEM, as ESP-IDF's tls::X509 holds them, see server::Configuration.
#[derive(Clone, Copy, Debug)]
pub struct X509(&'static [u8]);

impl X509 {
    /// The PEM in text, which ESP-IDF takes with its terminating nul, as a CStr.
    pub fn pem(text: &'static std::ffi::CStr) -> Self {
        X509(text.to_bytes())
    }

    pub fn pem_until_nul(bytes: &'static [u8]) -> Self {
        X509(bytes.split(|byte| *byte == 0).next().unwrap_or_default())
    }

    pub fn data(&self) -> &'static [u8] {
        self.0
    }
}

/// The name of an HTTP method, for the request line.
fn method_name(method: Method) -> &'static str {
    match method {
        Method::Delete => "DELETE",
        Method::Get => "GET",
        Method::Head => "HEAD",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Connect => "CONNECT",
        Method::Options => "OPTIONS",
        Method::Trace => "TRACE",
        Method::Patch => "PATCH",
        _ => "GET",
    }
}

/// The method named in a request line, if it is one of the usual ones.
fn method_named(name: &str) -> Option<Method> {
    Some(match name {
        "DELETE" => Method::Delete,
        "GET" => Method::Get,
        "HEAD" => Method::Head,
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "OPTIONS" => Method::Options,
        "PATCH" => Method::Patch,
        _ => return None,
    })
}

/// The raw connection, which the simulator's server and client do not give, see their raw_connection.
pub struct NoRawConnection;

impl io::ErrorType for NoRawConnection {
    type Error = SimError;
}

impl io::Read for NoRawConnection {
    fn read(&mut self, _buf: &mut [u8]) -> Result<usize, Self::Error> {
        Err(SimError::new("The simulator has no raw connection"))
    }
}

impl io::Write for NoRawConnection {
    fn write(&mut self, _buf: &[u8]) -> Result<usize, Self::Error> {
        Err(SimError::new("The simulator has no raw connection"))
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}
