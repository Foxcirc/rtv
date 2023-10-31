
//! This module contains definitions for the HTTP types that the user interacts with.
//!
//! It provides a [`RequestBuilder`] that allows constructing requests
//! as well as the [`Response`] type used to receive responses using a [`Client`](crate::Client).
//! The [`SimpleClient`](crate::SimpleClient) uses it's own response types.

use std::{fmt, time::Duration};

/// An HTTP method.
/// The default method is `GET`.
#[derive(Clone, Default)]
pub enum Method {
    #[default]
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
    Trace,
}

/// If the connection should use tls or not.
/// 
/// ```
/// Plain = HTTP
/// Secure = HTTPS
/// ```
///
#[derive(Clone, Default)]
pub enum Mode {
    #[default]
    Plain,
    #[cfg(feature = "tls")]
    Secure,
}

/// An HTTP URI.
/// The path may start with a `/` or it may not.
#[derive(Clone, Default)]
pub struct Uri<'a> {
    pub host: &'a str,
    pub path: &'a str,
}

/// An HTTP query.
#[derive(Clone)]
pub struct Query<'a> {
    pub name: &'a str,
    pub value: &'a str,
}

/// An HTTP header.
#[derive(Clone)]
pub struct Header<'a> {
    pub name: &'a str,
    pub value: &'a str,
}

/// The ID assigned to a request.
///
/// You can use it to check if a response belongs to a request.
///
/// The inner number will start at `0` and count up by `1` (wrapping) for every
/// request sent by a perticular client. You can rely on this behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReqId {
    pub inner: usize,
}

/// Used to build a request.
/// See [`Request`].
#[derive(Default, Clone)]
pub struct RequestBuilder<'a> {
    request: Request<'a>, // only partially populated
}

impl<'a> RequestBuilder<'a> {

    /// Sets the `timeout`.
    /// By default requests do not have a timeout.
    #[inline(always)]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.request.timeout = Some(timeout);
        self
    }

    #[inline(always)]
    pub fn method(mut self, method: Method) -> Self {
        self.request.method = method;
        self
    }

    /// Sets the `mode` to [`Mode::Secure`].
    #[cfg(feature = "tls")]
    #[inline(always)]
    pub fn secure(mut self) -> Self {
        self.request.mode = Mode::Secure;
        self
    }

    /// Alias to [`secure`](RequestBuilder::secure).
    #[cfg(feature = "tls")]
    #[inline(always)]
    pub fn https(self) -> Self {
        self.secure()
    }

    /// Set the uri.host component of this request.
    #[inline(always)]
    pub fn host(mut self, host: &'a str) -> Self {
        self.request.uri.host = host;
        self
    }

    /// Set the uri.path component of this request.
    #[inline(always)]
    pub fn path(mut self, path: &'a str) -> Self {
        self.request.uri.path = path;
        self
    }

    /// Add a query parameter to the path.
    ///
    /// # Example
    ///
    /// The uri `example.com?foo=1&bar=2` could be constructed
    /// using following code:
    /// `Request::build().host("example.com").query("foo", "1").query("bar", "2")`
    #[inline(always)]
    pub fn query(mut self, name: &'a str, value: &'a str) -> Self {
        self.request.queries.push(Query { name, value });
        self
    }

    /// Insert a header into this request.
    ///
    /// For information on which headers are managed by rtv, see the [`Request`] documentation.
    #[inline(always)]
    pub fn set(mut self, name: &'a str, value: &'a str) -> Self {
        self.request.headers.push(Header { name, value });
        self
    }

    /// Alias to [`set`](RequestBuilder::set).
    #[inline(always)]
    pub fn header(self, name: &'a str, value: &'a str) -> Self {
        self.set(name, value)
    }

    /// Insert the `User-Agent` header with the specified value.
    #[inline(always)]
    pub fn user_agent(self, value: &'a str) -> Self {
        self.set("User-Agent", value)
    }


    /// Update the request body with the specified data.
    #[inline(always)]
    pub fn send<T: AsRef<[u8]> + ?Sized>(mut self, body: &'a T) -> Self {
        self.request.body = body.as_ref();
        self
    }

    /// Get the request.
    /// You don't have to use this, since all functions that send a `Request` can also
    /// take a `RequestBuilder` directly.
    #[inline(always)]
    pub fn finish(self) -> Request<'a> {
        self.request
    }

}

/// This just calls [`finish`](RequestBuilder::finish).
impl<'a> From<RequestBuilder<'a>> for Request<'a> {
    #[inline(always)]
    fn from(builder: RequestBuilder<'a>) -> Self {
        builder.finish()
    }
}

/// Represents an HTTP request.
/// You can build a request either through this struct directly
/// or through a [`RequestBuilder`].
///
/// These headers will be set automatically:
/// - `Content-Length: ...`
/// - `Connection: close`
/// - `Accept-Encoding: identity`
///
/// You can overwrite the `Accept-Encoding` header
/// if you wanna receive encoded body data.
/// You cannot overwrite the other automatic headers.
///
/// # Example
///
/// Create a request using a builder.
///
/// ```rust
/// let req = Request::get().secure().host("example.com");
/// ```
///
/// Overwrite the `Accept-Encoding` header.
///
/// ```rust
/// let req = Request::get().set("Accept-Encoding", "gzip");
/// ```
///
/// Create a request directly,
/// although this is not recommended.
///
/// ```rust
/// let req = Request {
///     uri: Uri { host: "example.com", path: "" },
///     timeout: Some(Duration::from_secs(2)),
///     ..Default::default(),
/// };
/// ```
///
#[derive(Clone, Default)]
pub struct Request<'a> {
    pub timeout: Option<Duration>,
    pub method: Method,
    pub mode: Mode,
    pub uri: Uri<'a>,
    pub queries: Vec<Query<'a>>,
    pub headers: Vec<Header<'a>>,
    pub override_encoding: bool,
    pub override_charset: bool,
    pub body: &'a [u8],
}

impl<'a> Request<'a> {

    /// Build a request. For [`Method::Get`] and [`Method::Post`] there are two
    /// convencience functions.
    /// # Example
    /// Create a request using a builder and set the method to `Delete`.
    /// ```rust
    /// let req = Request::build().method(Method::Delete).host("example.com");
    /// ```
    /// Oh no we just deleted the exam-
    pub fn build() -> RequestBuilder<'a> {
        RequestBuilder::default()
    }

    /// Build a request with the `GET` method.
    /// 
    /// Other methods are available through [`Method`].
    pub fn get() -> RequestBuilder<'a> {
        RequestBuilder::default().method(Method::Get)
    }

    /// Build a request with the `POST` method.
    ///
    /// Other methods are available through [`Method`].
    pub fn post() -> RequestBuilder<'a> {
        RequestBuilder::default().method(Method::Post)
    }

    pub(crate) fn format(&self) -> Vec<u8> {

        let method = match self.method {
            Method::Get     => "GET",
            Method::Post    => "POST",
            Method::Put     => "PUT",
            Method::Delete  => "DELETE",
            Method::Patch   => "PATCH",
            Method::Head    => "HEAD",
            Method::Options => "OPTIONS",
            Method::Trace   => "TRACE",
        };

        let host = self.uri.host;
        let trimmed_path = self.uri.path.trim_start_matches("/");

        let mut path_builder = trimmed_path.to_string();
        for (idx, Query { name, value }) in self.queries.iter().enumerate() {
            path_builder += if idx == 0 { "?" } else { "&" };
            path_builder += name;
            path_builder += "=";
            path_builder += value;
        }

        let mut headers = String::new();
        let mut overwrite_encoding = false;
        let mut overwrite_charset = false;

        headers += "Content-Length: ";
        headers += &self.body.len().to_string();
        headers += "\r\n";

        headers += "Connection: close";
        headers += "\r\n";

        for Header { name, value } in self.headers.iter() {
            if *name == "Connection" || *name == "Content-Length" {
                panic!("The `{}` header is managed by rtv, for more info see the `Request` documentation", name);
            }
            else if *name == "Accept-Encoding" { overwrite_encoding = true }
            headers += name;
            headers += ": ";
            headers += value;
            headers += "\r\n";
        }

        if overwrite_encoding {
            headers += "Accept-Encoding: identity";
            headers += "\r\n";
        }

        let head = format!("{} /{} HTTP/1.1\r\nHost: {}\r\n{}\r\n", method, trimmed_path, host, headers);
        let mut bytes = head.into_bytes();

        bytes.extend_from_slice(self.body);

        bytes

    }

}

/// An owned HTTP header. This is used in a response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedHeader {
    pub name: String,
    pub value: String,
}

impl From<&httparse::Header<'_>> for OwnedHeader {
    fn from(header: &httparse::Header) -> Self {
        Self { name: header.name.to_string(), value: String::from_utf8_lossy(header.value).to_string() }
    }
}

/// A status code and message for a response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub code: u16,
    pub reason: String,
}

/// The `Head` of a response. This is not to be confused with an HTTP `Header`.
///
/// The response head contains informations about the response.
#[derive(Clone, PartialEq, Eq)]
pub struct ResponseHead {
    pub status: Status,
    pub headers: Vec<OwnedHeader>,
    // `0` if not present
    pub content_length: usize,
    // `true` if chunked transfer encoding is used
    pub transfer_chunked: bool,
}

impl ResponseHead {

    /// Get the value of a header. Returns `None` if the header could not be found.
    pub fn get_header<'d>(&'d self, name: &str) -> Option<&'d str> {
        self.headers.iter().find_map(Self::match_header(name))
    }

    /// Get an Iterator over all the headers.
    pub fn all_headers<'d>(&'d self, name: &'d str) -> impl Iterator<Item = &'d str> {
        self.headers.iter().filter_map(Self::match_header(name))
    }

    fn match_header<'d>(name: &'d str) -> impl for<'e> Fn(&'e OwnedHeader) -> Option<&'e str> + 'd {
        move |header| if header.name == name { Some(&header.value[..]) } else { None }
    }

}

impl fmt::Debug for ResponseHead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ResponseHead {{")?;
        writeln!(f, "    headers: [")?;
        for header in self.headers.iter() {
            writeln!(f, "        {}: {}", header.name, header.value)?;
        }
        writeln!(f, "    ]")?;
        writeln!(f, "    status: {:?}", self.status)?;
        writeln!(f, "    content_length: {:?}", self.content_length)?;
        writeln!(f, "    transfer_chunked: {:?}", self.transfer_chunked)?;
        write!(f, "}}")?;
        Ok(())
    }
}

/// An HTTP response.
/// Contains a [`ResponseState`].
///
/// A `Response` is **not** a full HTTP response but just one part of it. This arcitecture
/// allows for streaming the response data, not waiting for everything to arrive.
/// It also contains the response id, that could be obtained earlier when sending the request.
/// # Example
/// Here is an example of how matching against a response might look.
/// ```rust
/// match resp.state {
///     ResponseState::Head(head) => println!("content_length is {} bytes", head.content_length),
///     ResponseState::Data(some_data) => response_data_buffer.extend_from_slice(&some_data),
///     ...
/// }
/// ```
#[derive(Debug)]
pub struct Response {
    pub id: ReqId,
    pub state: ResponseState,
}

impl Response {

    pub(crate) fn new(id_num: usize, state: ResponseState) -> Self {
        Self { id: ReqId { inner: id_num }, state }
    }

}

/// The state of a response.
///
/// For more information see [`Request`].
/// The first thing you receive should be [`ResponseState::Head`], at least in a normal scenario.
/// If the request is finished and no error occured you will always receive [`ResponseState::Done`]
/// and no more events for that request afterwards.
#[derive(PartialEq, Eq)]
pub enum ResponseState {
    /// The response head. Contains information about what the response contains.
    Head(ResponseHead),
    /// We have read *some* data for this request. The data is not transmitted all at once,
    /// everytime the server sends a chunk of data you will receive one of these.
    Data(Vec<u8>),
    /// The request is done and will not generate any more events.
    Done,
    /// The request timed out. This will only occur if you set a timeout for a request.
    TimedOut,
    /// The server unexpectedly closed the connection for this request.
    Aborted,
    /// The host could not be found.
    UnknownHost,
    /// An error occured while reading the response. For example the server could've send invalid data.
    // todo: update these docs ^^
    Error,
}

impl ResponseState {

    /// Returns `true` if this state signals that the request is finished.
    ///
    /// This is literally implemented as:
    /// ```
    /// self.is_completed() || self.is_error()
    /// ```
    pub fn is_finished(&self) -> bool {
        self.is_done() || self.is_error()
    }

    /// Returns `true` if this state is `Done`.
    pub fn is_done(&self) -> bool {
        match self {
            Self::Head(..)    => false,
            Self::Data(..)    => false,
            Self::Done        => true,
            Self::TimedOut    => false,
            Self::Aborted        => false,
            Self::UnknownHost => false,
            Self::Error       => false,
        }
    }

    /// Returns `true` if this state is either `Dead`, `TimedOut`, `UnknownHost` or `Error`.
    pub fn is_error(&self) -> bool {
        match self {
            Self::Head(..)    => false,
            Self::Data(..)    => false,
            Self::Done        => false,
            Self::TimedOut    => true,
            Self::Aborted        => true,
            Self::UnknownHost => true,
            Self::Error       => true,
        }
    }

}

/// This doesn't print the `ResponseState::Data` raw or as a string, instread it just prints the length.
impl fmt::Debug for ResponseState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TimedOut => write!(f, "TimedOut"),
            Self::Head(head) => write!(f, "Head({:?})", head),
            Self::Data(data) => write!(f, "Data({} bytes)", data.len()),
            Self::Done => write!(f, "Done"),
            Self::Aborted => write!(f, "Dead"),
            Self::UnknownHost => write!(f, "UnknownHost"),
            Self::Error => write!(f, "Error"),
        }
    }
}

