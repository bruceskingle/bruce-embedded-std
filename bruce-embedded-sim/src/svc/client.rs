//! An HTTP client connection with ESP-IDF's client EspHttpConnection's shape, which embedded-svc's Client wraps, made
//! with ureq: a request is sent when its response is asked for, with the body written before, see
//! Connection::initiate_response, and connections are kept for the next request, as ESP-IDF's are.

use std::io::Read;
use std::time::Duration;

use embedded_svc::http::client::Connection;
use embedded_svc::http::{Headers, Method, Status};
use embedded_svc::io::{ErrorType, Read as SvcRead, Write as SvcWrite};

use super::{SimError, method_name};

/// The type of ESP-IDF's function which attaches its bundle of root certificates, which a Configuration gives to say
/// that the server's certificate is to be checked, see crt_bundle_attach.
pub type CrtBundleAttach = unsafe extern "C" fn(conf: *mut core::ffi::c_void) -> i32;

/// Stands for ESP-IDF's esp_crt_bundle_attach, for crt_bundle_attach: the simulator checks certificates against the
/// Mozilla roots that ESP-IDF's bundle is made from.
///
/// # Safety
///
/// It does nothing with its argument.
pub unsafe extern "C" fn esp_crt_bundle_attach(_conf: *mut core::ffi::c_void) -> i32 {
    0
}

/// How the connection is set up, with ESP-IDF's names, of which the simulator uses the timeout, and whether the
/// server's certificate is checked: only if crt_bundle_attach or use_global_ca_store is set, as on ESP-IDF, where a
/// connection without either takes any certificate, e.g. a Tesla gateway's own.
#[derive(Clone, Debug, Default)]
pub struct Configuration {
    pub buffer_size: Option<usize>,
    pub buffer_size_tx: Option<usize>,
    pub timeout: Option<Duration>,
    pub use_global_ca_store: bool,
    pub crt_bundle_attach: Option<CrtBundleAttach>,
    pub raw_request_body: bool,
}

/// The response's status and headers.
#[derive(Default)]
pub struct ResponseHead {
    status: u16,
    message: Option<String>,
    headers: Vec<(String, String)>,
}

impl Status for ResponseHead {
    fn status(&self) -> u16 {
        self.status
    }

    fn status_message(&self) -> Option<&'_ str> {
        self.message.as_deref()
    }
}

impl Headers for ResponseHead {
    fn header(&self, name: &str) -> Option<&'_ str> {
        self.headers
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// The response's body.
pub struct ResponseBody(Option<Box<dyn Read + Send>>);

impl ErrorType for ResponseBody {
    type Error = SimError;
}

impl SvcRead for ResponseBody {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        match &mut self.0 {
            Some(reader) => Ok(reader.read(buf)?),
            None => Err(SimError::new("There is no response to read")),
        }
    }
}

/// The request being made, before it is sent.
struct Pending {
    method: Method,
    uri: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

/// The connection, as ESP-IDF's client EspHttpConnection.
pub struct SimHttpConnection {
    agent: ureq::Agent,
    request: Option<Pending>,
    head: ResponseHead,
    body: ResponseBody,
    response_initiated: bool,
}

impl SimHttpConnection {
    pub fn new(configuration: &Configuration) -> Result<Self, SimError> {
        let verify = configuration.crt_bundle_attach.is_some() || configuration.use_global_ca_store;
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(configuration.timeout)
            .tls_config(ureq::tls::TlsConfig::builder().disable_verification(!verify).build())
            .build()
            .into();
        Ok(SimHttpConnection {
            agent,
            request: None,
            head: ResponseHead::default(),
            body: ResponseBody(None),
            response_initiated: false,
        })
    }

    /// Sends the pending request, keeping the response's head and body.
    fn send(&mut self, pending: Pending) -> Result<(), SimError> {
        let mut builder = ureq::http::Request::builder()
            .method(method_name(pending.method))
            .uri(&pending.uri);
        for (name, value) in &pending.headers {
            builder = builder.header(name, value);
        }
        let failed = |error: &dyn std::fmt::Display| {
            SimError::new(format!(
                "{} {} failed, {error}",
                method_name(pending.method),
                pending.uri
            ))
        };
        let response =
            if pending.body.is_empty() && matches!(pending.method, Method::Get | Method::Head | Method::Delete) {
                let request = builder.body(()).map_err(|error| failed(&error))?;
                self.agent.run(request)
            } else {
                let request = builder.body(pending.body.clone()).map_err(|error| failed(&error))?;
                self.agent.run(request)
            }
            .map_err(|error| failed(&error))?;
        self.head = ResponseHead {
            status: response.status().as_u16(),
            message: response.status().canonical_reason().map(String::from),
            headers: response
                .headers()
                .iter()
                .map(|(name, value)| (name.to_string(), String::from_utf8_lossy(value.as_bytes()).into_owned()))
                .collect(),
        };
        self.body = ResponseBody(Some(Box::new(response.into_body().into_reader())));
        Ok(())
    }
}

impl ErrorType for SimHttpConnection {
    type Error = SimError;
}

impl Status for SimHttpConnection {
    fn status(&self) -> u16 {
        self.head.status
    }

    fn status_message(&self) -> Option<&'_ str> {
        self.head.message.as_deref()
    }
}

impl Headers for SimHttpConnection {
    fn header(&self, name: &str) -> Option<&'_ str> {
        self.head.header(name)
    }
}

impl SvcRead for SimHttpConnection {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.body.read(buf)
    }
}

impl SvcWrite for SimHttpConnection {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        let pending = self
            .request
            .as_mut()
            .ok_or_else(|| SimError::new("There is no request to write to"))?;
        pending.body.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl Connection for SimHttpConnection {
    type Headers = ResponseHead;
    type Read = ResponseBody;
    type RawConnectionError = SimError;
    type RawConnection = super::NoRawConnection;

    fn initiate_request<'a>(
        &'a mut self,
        method: Method,
        uri: &'a str,
        headers: &'a [(&'a str, &'a str)],
    ) -> Result<(), Self::Error> {
        self.response_initiated = false;
        self.body = ResponseBody(None);
        self.request = Some(Pending {
            method,
            uri: uri.to_string(),
            headers: headers
                .iter()
                // Content-Length is set from the body, which ureq refuses to have set by hand.
                .filter(|(name, _)| !name.eq_ignore_ascii_case("Content-Length"))
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
            body: Vec::new(),
        });
        Ok(())
    }

    fn is_request_initiated(&self) -> bool {
        self.request.is_some()
    }

    fn initiate_response(&mut self) -> Result<(), Self::Error> {
        let pending = self
            .request
            .take()
            .ok_or_else(|| SimError::new("There is no request to send"))?;
        self.send(pending)?;
        self.response_initiated = true;
        Ok(())
    }

    fn is_response_initiated(&self) -> bool {
        self.response_initiated
    }

    fn split(&mut self) -> (&Self::Headers, &mut Self::Read) {
        (&self.head, &mut self.body)
    }

    fn raw_connection(&mut self) -> Result<&mut Self::RawConnection, Self::Error> {
        Err(SimError::new("The simulator's client has no raw connection"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svc::server::{Configuration as ServerConfiguration, SimHttpServer};
    use embedded_svc::http::client::Client;
    use embedded_svc::utils::io::try_read_full;

    #[test]
    fn a_request_is_answered_by_the_simulators_server() {
        let mut server = SimHttpServer::new(&ServerConfiguration {
            http_port: 0,
            ..Default::default()
        })
        .unwrap();
        server
            .fn_handler("/hello", Method::Post, |mut request| {
                let mut body = [0; 16];
                let read = request.read(&mut body)?;
                let mut response = request.into_response(200, None, &[("X-Test", "yes")])?;
                response.write(&body[..read])?;
                Ok::<(), SimError>(())
            })
            .unwrap();
        let mut client = Client::wrap(SimHttpConnection::new(&Configuration::default()).unwrap());
        let url = format!("http://127.0.0.1:{}/hello", server.port());
        let mut request = client.post(&url, &[("Content-Length", "2")]).unwrap();
        request.write(b"hi").unwrap();
        let mut response = request.submit().unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.header("x-test"), Some("yes"));
        let mut body = [0; 16];
        let read = try_read_full(&mut response, &mut body)
            .map_err(|(error, _)| error)
            .unwrap();
        assert_eq!(&body[..read], b"hi");
    }
}
