
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

/// An HTTP uri.
/// The path may start with a `/` or it may not.
#[derive(Clone, Default)]
pub struct Uri {
    pub host: String,
    pub path: String,
}

/// An HTTP header.
#[derive(Clone)]
pub struct Header {
    pub name: String,
    pub value: String,
}

/// The ID assigned to a request.
///
/// This should be treated as an opaque container.
/// You can use it to check if a response belongs to a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReqId {
    pub inner: usize,
}

/// Used to build a request.
/// See [`Request::build`].
#[derive(Default, Clone)]
pub struct RequestBuilder {
    request: Request, // partially populated
    queries: Vec<(String, String)>,
}

impl RequestBuilder {

    pub fn timeout(mut self, timeout: Option<Duration>) -> Self {
        self.request.timeout = timeout;
        self
    }

    pub fn method(mut self, method: Method) -> Self {
        self.request.method = method;
        self
    }

    pub fn mode(mut self, mode: Mode) -> Self {
        self.request.mode = mode;
        self
    }

    /// Sets `mode` to [`Mode::Secure`]
    #[cfg(feature = "tls")]
    pub fn secure(mut self) -> Self {
        self.request.mode = Mode::Secure;
        self
    }

    /// Set the uri.host component of this request.
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.request.uri.host = host.into();
        self
    }

    /// Set the uri.path component of this request.
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.request.uri.path = path.into();
        self
    }

    /// Add a query parameter to the path.
    ///
    /// # Example
    ///
    /// The uri `example.com?foo=1&bar=2` could be constructedusing
    /// using `Request::build().host("example.com).`
    pub fn query(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.queries.push((name.into(), value.into()));
        self
    }

    /// Insert a header into this request.
    pub fn set(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        let name_string = name.into();
        if matches!(&name_string[..], "Connection" | "Accept-Encoding" | "Content-Length") {
            panic!("The `{}` header is managed by rtv, for more info see the `Request` documentation", name_string);
        }
        self.request.headers.push(Header { name: name_string, value: value.into() });
        self
    }

    /// Update the request body with the specified data.
    pub fn send_bytes(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.request.body = body.into();
        self
    }

    /// Like [`send_bytes`](RequestBuilder::send_bytes).
    /// Convenience function to convert the string to bytes for you.
    pub fn send_str(mut self, body: impl Into<String>) -> Self {
        self.request.body = body.into().into_bytes();
        self
    }

    /// Get the request.
    /// You don't *have* to use this, since all functions that send a request can also
    /// take a `RequestBuilder` directly.
    pub fn finish(self) -> Request {
        let mut request = self.request;
        let mut param_str = String::new();
        for (idx, (name, value)) in self.queries.into_iter().enumerate() {
            param_str += if idx == 0 { "?" } else { "&" };
            param_str += &name;
            param_str += "=";
            param_str += &value;
        }
        request.uri.path += &param_str;
        request
    }

}

/// This just calls [`finish`](RequestBuilder::finish).
impl From<RequestBuilder> for Request {
    fn from(builder: RequestBuilder) -> Self {
        builder.finish()
    }
}

/// Represents an HTTP request.
/// You can build a request either through this struct directly
/// or through a [`RequestBuilder`].
///
/// Three headers will be set automatically:
/// - `Connection: close`
/// - `Accept-Encoding: identity`
/// - `Content-Length: body.len()`
///
/// # Example
///
/// Create a request using a builder.
///
/// ```rust
/// let req = Request::get().host("example.com").secure();
/// ```
///
/// Create a request directly.
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
pub struct Request {
    pub timeout: Option<Duration>,
    pub method: Method,
    pub mode: Mode,
    pub uri: Uri,
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
}

impl Request {

    /// Build a request. For [`Method::Get`] and [`Method::Post`] there are two
    /// convencience functions.
    /// # Example
    /// Create a request using a builder and set the method to `Delete`.
    /// ```rust
    /// let req = Request::build().method(Method::Delete).host("example.com");
    /// ```
    /// Oh no we just deleted the exam-
    pub fn build() -> RequestBuilder {
        RequestBuilder::default()
    }

    /// Build a request with the `GET` method.
    /// 
    /// Other methods are available through [`Method`].
    pub fn get() -> RequestBuilder {
        RequestBuilder::default().method(Method::Get)
    }

    /// Build a request with the `POST` method.
    ///
    /// Other methods are available through [`Method`].
    pub fn post() -> RequestBuilder {
        RequestBuilder::default().method(Method::Post)
    }

    /// Format the request into valid HTTP plaintext.
    ///
    /// This is for internal use but exposed because it might be useful.
    pub fn format(&self) -> Vec<u8> {

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

        let path = &self.uri.path;
        let host = &self.uri.host;

        let mut headers = String::new();
        for Header { name, value } in self.headers.iter() {
            headers += name;
            headers += ": ";
            headers += value;
            headers += "\r\n";
        }

        headers += "Content-Length: ";
        headers += &self.body.len().to_string();
        headers += "\r\n";

        headers += "Connection: close";
        headers += "\r\n";

        headers += "Accept-Encoding: identity";
        headers += "\r\n";

        let trimmed_path = path.trim_start_matches('/');

        let head = format!("{} /{} HTTP/1.1\r\nHost: {}\r\n{}\r\n", method, trimmed_path, host, headers);
        let mut bytes = head.into_bytes();

        bytes.extend_from_slice(&self.body);

        bytes

    }

}

/// An owned HTTP header. This is used in a response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedHeader {
    pub name: String,
    pub value: String,
}

/// A status code and message for a response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub code: u16,
    pub text: String,
}

/// The `Head` of a response. This is not to be confused with an HTTP `Header`.
///
/// The response head contains informations about the response.
/// It contains the status, the headers and the content_length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseHead {
    pub status: Status,
    pub headers: Vec<OwnedHeader>,
    pub content_length: usize,
    pub transfer_chunked: bool,
}

impl ResponseHead {

    /// Get the value of a header. Returns `None` if the header could not be found.
    pub fn get_header<'d>(&'d self, name: &str) -> Option<&'d str> {
        self.headers.iter().find_map(move |header| if header.name == name { Some(&header.value[..]) } else { None } )
    }

    /// Get an Iterator over all the headers.
    pub fn all_headers<'d>(&'d self, name: &'d str) -> impl Iterator<Item = &'d str> {
        self.headers.iter().filter_map(move |header| if header.name == name { Some(&header.value[..]) } else { None })
    }

    pub(crate) fn parse(bytes: &[u8]) -> Option<(Self, usize)> {

        let head_end = bytes.windows(4).position(|bytes| bytes == &[0x0D, 0x0A, 0x0D, 0x0A] /* double CRLF */)?;

        let head_raw = &bytes[..head_end];
        let head = String::from_utf8(head_raw.to_vec()).ok()?;
        let mut lines = head.lines();
        
        let mut info = lines.next()?.splitn(3, ' ');
        let _http_version = info.next()?;
        let status_code = info.next()?.parse::<u16>().ok()?;
        let status_text = info.next()?;

        let mut content_length = 0;
        let mut transfer_chunked = false;

        let mut headers = Vec::with_capacity(8);
        for line in lines {

            let (name, value) = line.split_once(':')?;
            let name = name.trim();
            let value = value.trim();

            if name == "Content-Length" { content_length = value.parse().ok()? }
            if name == "Transfer-Encoding" && value == "chunked" { transfer_chunked = true }

            headers.push(OwnedHeader { name: name.to_string(), value: value.to_string() })

        }

        let status = Status {
            code: status_code,
            text: status_text.to_string(),
        };
        
        Some((Self {
            status,
            headers,
            content_length,
            transfer_chunked,
        }, head_end + 4 /* skip the seperator */))

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
    Dead,
    /// The host could not be found.
    UnknownHost,
    /// An error occured while reading the response. For example the server could've send invalid data.
    Error,
}

impl ResponseState {

    /// Returns `true` if this state is either `Dead`, `TimedOut`, `UnknownHost` or `Error`.
    pub fn is_error(&self) -> bool {
        match self {
            Self::Head(..) => false,
            Self::Data(..) => false,
            Self::Done => false,
            Self::TimedOut => true,
            Self::Dead => true,
            Self::UnknownHost => true,
            Self::Error => true,
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
            Self::Dead => write!(f, "Dead"),
            Self::UnknownHost => write!(f, "UnknownHost"),
            Self::Error => write!(f, "Error"),
        }
    }
}

