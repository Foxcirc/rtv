
use std::{fmt, io, iter::zip, time::Duration, error, string, borrow::Cow};

use crate::{Client, Request, ResponseHead, ResponseState};

/// A simpler HTTP client that handles I/O events for you.
///
/// The `SimpleClient` allows you to:
///     1. Send a [`single`](SimpleClient::send) request and block until the response has arrived.
///     2. Send [`many`](SimpleClient::many) requests at the same time and block until all responses have arrived.
///     3. Send a single request and [`stream`](SimpleClient::stream) the request body.
///
/// # Example
///
/// It is really easy to send a single request.
///
/// ```rust
/// let mut client = SimpleClient::new()?;
/// let resp = client.send(Request::get("example.com"))?;
/// let body_str = String::from_utf8(resp.body); // note: not all websites use UTF-8!
/// println!("{}", body_str);
/// ```
pub struct SimpleClient {
    io: mio::Poll,
    client: Client,
    next_id: usize,
}

impl SimpleClient {

    /// Creates a new client
    ///
    /// The result maybe an IO error produced by `mio`.
    pub fn new() -> RequestResult<Self> {

        Ok(Self {
            io: mio::Poll::new()?,
            client: Client::new(mio::Token(0)),
            next_id: 1,
        })

    }

    /// Send a single request.
    ///
    /// This method will send a single request and block until the
    /// response arrives.
    pub fn send<'a>(&mut self, input: impl Into<Request<'a>>) -> RequestResult<SimpleResponse<Vec<u8>>> {

        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        let request: Request = input.into();
        let timeout = request.timeout;

        self.client.send(&self.io, mio::Token(id), request)?;

        let mut events = mio::Events::with_capacity(4);

        let mut response_head = None;
        let mut result = Vec::with_capacity(128);

        'ev: loop {

            self.io.poll(&mut events, timeout)?;

            for response in self.client.pump(&self.io, &events)? {
                match response.state {
                    ResponseState::Head(head) => response_head = Some(head),
                    ResponseState::Data(mut some_data) => result.append(&mut some_data),
                    ResponseState::Done => break 'ev,
                    ResponseState::Dead => return Err(RequestError::Dead),
                    ResponseState::TimedOut => return Err(RequestError::TimedOut),
                    ResponseState::UnknownHost => return Err(RequestError::UnknownHost),
                    ResponseState::Error => return Err(RequestError::Error),
                }
            }

            events.clear();

        }

        let head = response_head.expect("no response head");
        Ok(SimpleResponse{ head, body: result })

    }

    // todo: make a function that returns a Future

    /// Stream a single request.
    ///
    /// This method will send a single request and return a response once the
    /// [`ResponseHead`] has been transmitted.
    /// The response will contain a [`BodyReader`] as the `body` which implements
    /// the [`Read`](std::io::Read) trait.
    ///
    /// You can receive large responses packet-by-packet using this method.
    pub fn stream<'a, 'd>(&'d mut self, input: impl Into<Request<'a>>) -> RequestResult<SimpleResponse<BodyReader<'d>>> {

        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        let request: Request = input.into();
        let timeout = request.timeout;

        self.client.send(&self.io, mio::Token(id), request)?;

        let mut events = mio::Events::with_capacity(2);

        let mut response_head = None;
        let mut response_data_buffer = Vec::with_capacity(128);
        let mut is_done = false;

        'ev: loop {

            let mut stop = false;
            self.io.poll(&mut events, timeout)?;

            for response in self.client.pump(&self.io, &events)? {
                match response.state {
                    ResponseState::Head(head) => { response_head = Some(head); stop = true },
                    // we need to process all the response states, since the whole request
                    // might have been transmitted in one packet
                    ResponseState::Data(mut some_data) => response_data_buffer.append(&mut some_data),
                    ResponseState::Done => { is_done = true; stop = true },
                    ResponseState::Dead => return Err(RequestError::Dead),
                    ResponseState::TimedOut => return Err(RequestError::TimedOut),
                    ResponseState::UnknownHost => return Err(RequestError::UnknownHost),
                    ResponseState::Error => return Err(RequestError::Error),
                }
            }

            if stop { break 'ev }

            events.clear();

        }

        let head = response_head.expect("No header?");

        let reader = BodyReader {
            io: &mut self.io,
            client: &mut self.client,
            events: mio::Events::with_capacity(4),
            timeout,
            storage: response_data_buffer,
            is_done,
        };

        return Ok(SimpleResponse { head, body: reader });

    }

    /// Send many requests.
    ///
    /// This method allows sending multiple requests and waiting for them to
    /// resolve all at the same time.
    /// 
    /// The responses will be in the same order as the requests and the number
    /// of responses will always be the same as the number of requests.
    ///
    /// # Example
    ///
    /// ```rust
    /// let reqs = vec![Request::get().host("example.com"), Request::get().host("wikipedia.org")];
    /// let mut client = SimpleClient::new()?;
    /// let resps = client.send(reqs)?;
    /// resps[0]? // belongs to example.com
    /// resps[1]? // belongs to wikipedia.org
    /// assert!(resps.len() == reqs.len());
    /// ```
    pub fn many<'a>(&mut self, input: Vec<impl Into<Request<'a>>>) -> RequestResult<Vec<RequestResult<SimpleResponse<Vec<u8>>>>> {

        // todo: this code kinda dirty
        // todo: also enable taking a [Request; N]
        let num_requests = input.len();

        let mut response_builders = vec![Some(ResponseBuilder { head: None, body: Vec::with_capacity(128) }); num_requests];
        let mut responses = Vec::with_capacity(num_requests);
        for _ in 0..num_requests { responses.push(Err(RequestError::Dead)) }

        for input_item in input.into_iter() {

            let id = self.next_id;
            self.next_id = self.next_id.wrapping_add(1);

            let request: Request = input_item.into();

            self.client.send(&self.io, mio::Token(id), request)?;

        }

        let mut events = mio::Events::with_capacity(num_requests * 2);
        let mut counter = 0;

        'ev: loop {

            self.io.poll(&mut events, self.client.timeout())?;

            for response in self.client.pump(&self.io, &events)? {
                
                // we can use the response id as an index into the response array since it
                // will be counting up from 0
                // this is an implementation detail and as such easy to break but it's not like I'm
                // gonna change the way the indexing works internally
                let idx = response.id.inner;

                match response.state {
                    ResponseState::Head(head) => response_builders[idx].as_mut().expect("No builder.").head = Some(head),
                    ResponseState::Data(mut some_data) => response_builders[idx].as_mut().expect("No builder.").body.append(&mut some_data),
                    ResponseState::Done | ResponseState::Dead | ResponseState::TimedOut | ResponseState::UnknownHost | ResponseState::Error => {
                        let result = match response.state {
                            ResponseState::Done => {
                                let builder = response_builders[idx].take().expect("No builder.");
                                let head = builder.head.expect("No head?!");
                                Ok(SimpleResponse { head, body: builder.body })
                            },
                            ResponseState::Dead => Err(RequestError::Dead),
                            ResponseState::TimedOut => Err(RequestError::TimedOut),
                            ResponseState::UnknownHost => Err(RequestError::UnknownHost),
                            ResponseState::Error => Err(RequestError::Error),
                            _ => unreachable!(),
                        };
                        responses[idx] = result;
                        counter += 1;
                        if counter == num_requests { break 'ev }
                    },
                }

            }

            events.clear();

        }

        Ok(responses)

    }

}

#[derive(Clone)]
struct ResponseBuilder {
    pub(crate) head: Option<ResponseHead>,
    pub(crate) body: Vec<u8>,
}

/// Allows streaming the body of a request.
///
/// This does some internal buffering.
/// For more information see [`SimpleClient::stream`].
pub struct BodyReader<'b> {
    pub(crate) io: &'b mut mio::Poll,
    pub(crate) client: &'b mut Client,
    pub(crate) events: mio::Events,
    pub(crate) timeout: Option<Duration>,
    pub(crate) storage: Vec<u8>,
    pub(crate) is_done: bool,
}

impl<'b> io::Read for BodyReader<'b> {

    fn read(&mut self, buff: &mut [u8]) -> io::Result<usize> {

        if self.storage.is_empty() && !self.is_done {

            'ev: loop {

                let mut stop = false;
                self.io.poll(&mut self.events, self.timeout)?;

                for response in self.client.pump(&self.io, &self.events)? {
                    
                    match response.state {
                        ResponseState::Data(mut some_data) => { self.storage.append(&mut some_data); stop = true },
                        ResponseState::Done => { self.is_done = true; stop = true },
                        ResponseState::Dead => return Err(io::Error::new(io::ErrorKind::ConnectionAborted, RequestError::Dead)),
                        ResponseState::TimedOut => return Err(io::Error::new(io::ErrorKind::TimedOut, RequestError::TimedOut)),
                        ResponseState::UnknownHost => return Err(io::Error::new(io::ErrorKind::Other, RequestError::UnknownHost)),
                        ResponseState::Error => return Err(io::Error::new(io::ErrorKind::Other, RequestError::Error)),
                        ResponseState::Head(..) => unreachable!(),
                    }

                }

                if stop { break 'ev }

                self.events.clear();

            }

        }

        let bytes_to_read = buff.len().min(self.storage.len());
        let data = self.storage.drain(..bytes_to_read);

        for (src, dst) in zip(data, buff) { *dst = src };

        Ok(bytes_to_read)

    }

}

/// A simple response.
///
/// This cannot be errornous at protocol level.
/// The `body` may be a [`Vec<u8>`](std::vec::Vec), or a [`BodyReader`].
#[derive(Clone)]
pub struct SimpleResponse<B> {
    pub head: ResponseHead,
    pub body: B,
}

impl SimpleResponse<Vec<u8>> {

    /// Convert the request body into a `String`.
    /// Note that the data is assumed to be valid utf8. Text encodings
    /// are not handeled by this crate.
    pub fn into_string(self) -> Result<String, string::FromUtf8Error> {
        String::from_utf8(self.body)
    }

    /// Access the request body as a `&str`.
    pub fn to_str<'d>(&'d self) -> Result<&'d str, std::str::Utf8Error> {
        std::str::from_utf8(&self.body)
    }

}

impl<B> fmt::Debug for SimpleResponse<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SimpleResponse {{ ... }}") // todo: print information about the head
    }
}

pub type RequestResult<T> = Result<T, RequestError>;

/// An error that may occur when sending a request.
#[derive(Debug)]
pub enum RequestError {
    /// An IO error occured, such as a connection loss.
    IO(io::Error),
    /// The server unexpectedly closed the connection.
    Dead,
    /// The request timed out. Can only occur if you set a timeout for a request.
    TimedOut,
    /// The host (for example "google.com") could not be found.
    UnknownHost,
    /// Another error occured.
    Error,
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IO(err) => write!(f, "I/O Error: {}", err),
            Self::Dead => write!(f, "Dead: The server closed the connection unexpectedly."),
            Self::TimedOut => write!(f, "TimedOut: Request timed out."),
            Self::UnknownHost => write!(f, "UnknownHost: The host's IP address could not be resolved."),
            Self::Error => write!(f, "Error: There was an error receiving the request."),
        }
    }
}

impl error::Error for RequestError {}

impl From<io::Error> for RequestError {
    fn from(value: io::Error) -> Self {
        Self::IO(value)
    }
}

