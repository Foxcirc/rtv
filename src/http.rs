
use std::{fmt, str::FromStr, time::Duration};

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

#[derive(Clone, Default)]
pub struct Uri<'a> {
    pub host: &'a str,
    pub path: &'a str,
}

#[derive(Clone)]
pub struct Header<'a> {
    pub name: &'a str,
    pub value: &'a str,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReqId {
    pub(crate) inner: usize,
}

impl fmt::Debug for ReqId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ReqId(..)")
    }
}

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

    pub fn uri(mut self, host: &'a str, path: &'a str) -> Self {
        self.request.uri = Uri { host, path };
        self
    }

    pub fn set(mut self, name: &'a str, value: &'a str) -> Self {
        self.request.headers.push(Header { name, value });
        self
    }

    pub fn send_str(mut self, body: &'a str) -> Self {
        self.request.body = body.as_bytes();
        self
    }

    pub fn send_bytes(mut self, body: &'a [u8]) -> Self {
        self.request.body = body;
        self
    }

    pub fn finish(self) -> Request<'a> {
        self.request
    }

}

impl<'a> From<RequestBuilder<'a>> for Request<'a> {
    fn from(builder: RequestBuilder<'a>) -> Self {
        builder.finish()
    }
}

#[derive(Clone, Default)]
pub struct Request<'a> {
    pub timeout: Option<Duration>,
    pub method: Method,
    pub uri: Uri<'a>,
    pub headers: Vec<Header<'a>>,
    pub body: &'a [u8],
}

impl<'a> Request<'a> {

    pub fn build() -> RequestBuilder<'a> {
        RequestBuilder::default()
    }

    pub fn get() -> RequestBuilder<'a> {
        RequestBuilder::default().method(Method::Get)
    }

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

        let trimmed_path = path.trim_start_matches('/');

        let head = format!("{} /{} HTTP/1.1\nHost: {}\n{}\n", method, trimmed_path, host, headers);
        let mut head_bytes = head.into_bytes();

        head_bytes.extend_from_slice(self.body);

        head_bytes

    }

}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub code: u16,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseHead {
    pub status: Status,
    pub headers: Vec<OwnedHeader>,
    pub content_length: usize,
}

impl ResponseHead {

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

        let mut headers = Vec::with_capacity(8);
        for line in lines {

            let (name, value) = line.split_once(':')?;

            if name == "Content-Length" { content_length = value.trim().parse().ok()? }

            headers.push(OwnedHeader { name: name.trim().to_string(), value: value.trim().to_string() })

        }

        let status = Status {
            code: status_code,
            text: status_text.to_string(),
        };
        
        Some((Self {
            status,
            headers,
            content_length,
        }, head_end + 4 /* skip the seperator */))

    }

}

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

#[derive(PartialEq, Eq)]
pub enum ResponseState {
    TimedOut,
    Head(ResponseHead),
    Data(Vec<u8>),
    Done,
    Dead,
}

impl fmt::Debug for ResponseState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TimedOut => write!(f, "TimedOut"),
            Self::Head(head) => write!(f, "Head({:?})", head),
            Self::Data(data) => write!(f, "Data({} bytes)", data.len()),
            Self::Done => write!(f, "Done"),
            Self::Dead => write!(f, "Dead"),
        }
    }
}

pub trait Headers {
    fn get_header<T: FromStr>(&self, name: &str) -> Option<T>;
    fn all_headers<'d>(&'d self, name: &str) -> Vec<&'d str>;
}

impl Headers for ResponseHead {
    fn get_header<T: FromStr>(&self, name: &str) -> Option<T> {
        self.headers.iter().find_map(|header| if header.name == name { Some(header.value.parse().ok()?) } else { None } )
    }
    fn all_headers<'d>(&'d self, name: &str) -> Vec<&'d str> {
        self.headers.iter().filter_map(|header| if header.name == name { Some(&header.value[..]) } else { None }).collect()
    }
}

