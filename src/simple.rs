
//! This module contains a [`SimpleClient`] that allows sending simple requests.

use std::{fmt, io::{self, Read}, string};

use crate::{Client, Request, ResponseHead, ResponseState, Status};

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
    pub fn new() -> io::Result<Self> {

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
    pub fn send<'a>(&mut self, input: impl Into<Request<'a>>) -> io::Result<SimpleResponse<Vec<u8>>> {

        let mut response = self.stream(input)?;
        let mut buff = Vec::with_capacity(2048);
        response.body.read_to_end(&mut buff)?;
        Ok(SimpleResponse {
            head: response.head,
            body: buff,
        })

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
    pub fn stream<'a, 'd>(&'d mut self, input: impl Into<Request<'a>>) -> io::Result<SimpleResponse<BodyReader<'d>>> {

        let request: Request = input.into();
        let mut events = mio::Events::with_capacity(2);
        let mut result = SimpleResponse::empty();
        let mut is_done = false;

        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        self.client.send(&self.io, mio::Token(id), request)?;

        let mut stop = false;

        loop {

            self.io.poll(&mut events, self.client.timeout())?;

            for response in self.client.pump(&self.io, &events)? {
                match response.state {
                    ResponseState::Head(head) => {
                        result.head = head;
                        stop = true;
                    },
                    // we need to process all the response states, since the whole request
                    // might have been transmitted at once (but split into multiple ResponseStates)
                    ResponseState::Data(some_data) => result.body.extend(some_data),
                    ResponseState::Done => {
                        is_done = true;
                        stop = true;
                    },
                    ResponseState::Aborted     => return Err(io::Error::from(io::ErrorKind::ConnectionAborted)),
                    ResponseState::TimedOut    => return Err(io::Error::from(io::ErrorKind::TimedOut)),
                    ResponseState::UnknownHost => return Err(io::Error::new(io::ErrorKind::Other, "unknown host")),
                    ResponseState::Error       => return Err(io::Error::new(io::ErrorKind::Other, "http protocol error")),
                }
            }

            // we need to check this here because we have to process all the responses
            if stop {
                break
            }

            events.clear();

        }

        let reader = BodyReader {
            io: &mut self.io,
            client: &mut self.client,
            events: mio::Events::with_capacity(4),
            storage: result.body,
            is_done,
        };

        return Ok(SimpleResponse { head: result.head, body: reader });

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
    ///
    /// # Note
    /// This method is currently pretty inefficient because
    /// - since everything is sent at the same time, there will be one dns resolution for every request
    /// - since this is http 1.1 (without multiplexing) there will be one tcp stream for every request
    // todo: add the possibility to resolve the dns address yourself (using the `dns` module)
    pub fn many<'a>(&mut self, input: impl IntoIterator<Item = impl Into<Request<'a>>>) -> io::Result<Vec<io::Result<SimpleResponse<Vec<u8>>>>> {

        let iter = input.into_iter();
        let (min_size, _max_size) = iter.size_hint();

        let mut responses = Vec::with_capacity(min_size);
        let mut events = mio::Events::with_capacity(min_size * 2);

        for item in iter {

            let request = item.into();
            responses.push(Ok(SimpleResponse::empty()));

            let id = self.next_id;
            self.next_id = self.next_id.wrapping_add(1);

            self.client.send(&self.io, mio::Token(id), request)?;

        }

        let mut done = 0;

        loop {

            self.io.poll(&mut events, self.client.timeout())?;

            for response in self.client.pump(&self.io, &events)? {
                match response.state {
                    ResponseState::Head(head)  => if let Ok(val) = &mut responses[response.id.inner] { val.head = head },
                    ResponseState::Data(data)  => if let Ok(val) = &mut responses[response.id.inner] { val.body.extend(data) },
                    ResponseState::Done        => done += 1,
                    ResponseState::Aborted     => responses[response.id.inner] = Err(io::Error::from(io::ErrorKind::ConnectionAborted)),
                    ResponseState::TimedOut    => responses[response.id.inner] = Err(io::Error::from(io::ErrorKind::TimedOut)),
                    ResponseState::UnknownHost => responses[response.id.inner] = Err(io::Error::new(io::ErrorKind::Other, "unknown host")),
                    ResponseState::Error       => responses[response.id.inner] = Err(io::Error::new(io::ErrorKind::Other, "http protocol error")),
                }
            }

            if done == responses.len() {
                break
            }

            events.clear();

        }

        Ok(responses)

    }

}

/// Allows streaming the body of a request.
///
/// This does some internal buffering.
/// For more information see [`SimpleClient::stream`].
pub struct BodyReader<'b> {
    pub(crate) io: &'b mut mio::Poll,
    pub(crate) client: &'b mut Client,
    pub(crate) events: mio::Events,
    pub(crate) storage: Vec<u8>,
    pub(crate) is_done: bool,
}

impl<'b> io::Read for BodyReader<'b> {

    fn read(&mut self, buff: &mut [u8]) -> io::Result<usize> {

        if self.storage.is_empty() && !self.is_done {

            'ev: loop {

                let mut stop = false;
                self.io.poll(&mut self.events, self.client.timeout())?;

                for response in self.client.pump(&self.io, &self.events)? {
                    
                    match response.state {
                        ResponseState::Head(..) => unreachable!(),
                        ResponseState::Data(some_data) => {
                            self.storage.extend(some_data);
                            stop = true
                        },
                        ResponseState::Done        => {
                            self.is_done = true;
                            stop = true
                        },
                        ResponseState::Aborted     => return Err(io::Error::from(io::ErrorKind::ConnectionAborted)),
                        ResponseState::TimedOut    => return Err(io::Error::from(io::ErrorKind::TimedOut)),
                        ResponseState::UnknownHost => return Err(io::Error::new(io::ErrorKind::Other, "unknown host")),
                        ResponseState::Error       => return Err(io::Error::new(io::ErrorKind::Other, "http protocol error")),
                    }

                }

                if stop {
                    break 'ev
                }

                self.events.clear();

            }

        }

        let bytes_to_return = buff.len().min(self.storage.len());
        buff[..bytes_to_return].copy_from_slice(&self.storage[..bytes_to_return]);
        self.storage.drain(..bytes_to_return);

        Ok(bytes_to_return)

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

    fn empty() -> Self {
        Self {
            head: ResponseHead {
                status: Status { code: 0, reason: String::new() },
                headers: Vec::new(),
                content_length: 0,
                transfer_chunked: false
            },
            body: Vec::new(),
        }
    }

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

