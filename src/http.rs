
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

/// An HTTP uri.
/// The path may start with a `/` or it may not, this is handeled internally.
#[derive(Clone, Default)]
pub struct Uri<'a> {
    pub host: &'a str,
    pub path: &'a str,
}

/// An HTTP header.
#[derive(Clone)]
pub struct Header<'a> {
    pub name: &'a str,
    pub value: &'a str,
}

/// The ID assigned to a request.
///
/// This should be treated as an opaque container.
/// You can use it to check if a response belongs to a request.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReqId {
    pub inner: usize,
}

impl fmt::Debug for ReqId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ReqId(..)")
    }
}

/// Used to build a request.
/// See [`Request::build`].
#[derive(Default, Clone)]
pub struct RequestBuilder<'a> {
    request: Request<'a>,
}

impl<'a> RequestBuilder<'a> {

    pub fn timeout(mut self, timeout: Option<Duration>) -> Self {
        self.request.timeout = timeout;
        self
    }

    pub fn method(mut self, method: Method) -> Self {
        self.request.method = method;
        self
    }

    /// Set the uri.host component of this request.
    pub fn host(mut self, host: &'a str) -> Self {
        self.request.uri.host = host;
        self
    }

    /// Set the uri.path component of this request.
    pub fn path(mut self, path: &'a str) -> Self {
        self.request.uri.path = path;
        self
    }

    /// Insert a header into this request.
    pub fn set(mut self, name: &'a str, value: &'a str) -> Self {
        self.request.headers.push(Header { name, value });
        self
    }

    /// Update the request body with the specified data.
    pub fn send_bytes(mut self, body: &'a [u8]) -> Self {
        self.request.body = body;
        self
    }

    /// Like [`send_bytes`](RequestBuilder::send_bytes).
    /// Convenience function to convert the string to bytes for you.
    pub fn send_str(mut self, body: &'a str) -> Self {
        self.request.body = body.as_bytes();
        self
    }

    /// Get the request.
    /// You don't *have* to use this, since all functions that send a request can also
    /// take a `RequestBuilder` directly.
    pub fn finish(self) -> Request<'a> {
        self.request
    }

}

/// This just calls [`finish`](RequestBuilder::finish).
impl<'a> From<RequestBuilder<'a>> for Request<'a> {
    fn from(builder: RequestBuilder<'a>) -> Self {
        builder.finish()
    }
}

/// Represents an HTTP request.
/// You can build a request either through this struct directly
/// or through a [`RequestBuilder`].
/// # Example
/// Create a request directly.
/// ```rust
/// let req = Request {
///     uri: Uri { host: "example.com", path: "" },
///     ..Default::default(),
/// };
/// ```
/// Create a request using a builder.
/// ```rust
/// let req = Request::get().host("example.com");
/// ```
#[derive(Clone, Default)]
pub struct Request<'a> {
    pub timeout: Option<Duration>,
    pub method: Method,
    pub uri: Uri<'a>,
    pub headers: Vec<Header<'a>>,
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
    pub fn get() -> RequestBuilder<'a> {
        RequestBuilder::default().method(Method::Get)
    }

    /// Build a request with the `POST` method.
    pub fn post() -> RequestBuilder<'a> {
        RequestBuilder::default().method(Method::Post)
    }

    pub(crate) fn format(&self) -> Vec<u8> {

        let method = match self.method {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
            Method::Patch => "PATCH",
            Method::Head => "HEAD",
            Method::Options => "OPTIONS",
            Method::Trace => "TRACE",
        };

        let path = self.uri.path;
        let host = self.uri.host;

        let mut headers = String::new();
        for Header { name, value } in self.headers.iter() {
            headers += name;
            headers += ": ";
            headers += value;
            headers += "\n";
        }

        headers += "Content-Length: ";
        headers += &self.body.len().to_string();
        headers += "\n";

        headers += "Accept-Encoding: identity";
        headers += "\n";

        let trimmed_path = path.trim_start_matches('/');

        let head = format!("{} /{} HTTP/1.1\nHost: {}\n{}\n", method, trimmed_path, host, headers);
        let mut head_bytes = head.into_bytes();

        head_bytes.extend_from_slice(self.body);

        head_bytes

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
    /// The request timed out. This will only occur when you set a timeout for a request.
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

