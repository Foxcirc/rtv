
use std::{io::{self, Write, Read}, time::{Duration, Instant}, collections::HashMap, net::{SocketAddr, Ipv4Addr}};
#[cfg(feature = "tls")]
use std::sync::Arc;
use mio::net::TcpStream;
use crate::{dns, util::{make_socket_addr, notconnected, register_all, wouldblock, is_elapsed}, ResponseHead, Request, ReqId, Response, ResponseState, Mode};

/// A flexible HTTP client.
///
/// Use the client if you wanna have controll over `mio` yourself.
/// You should look at the documentation of the individual methods for more info on
/// what exactly they do.
///
/// In general, you pass the client a handle to you `Poll` when you send a request.
/// Inside you `mio` event loop you then call the `pump` function, which drives the request
/// to completion.
///
/// # Example
///
/// This is more or less a full blown example on what it takes to correctly
/// send and receive a request.
///
/// ```rust
///
/// let io = mio::Poll::new()?;
/// let mut client = rtv::Client::new(mio::Token(0));
///
/// let request = Request::get().host("example.com").https();
/// let _id = client.send(&io, mio::Token(2), request)?;
///
/// let mut response_body = Vec::new();
///
/// 'ev: loop {
///     
///     io.poll(&mut events, None)?;
///     
///     for resp in client.pump(&io, &events)? {
///         match resp.state {
///             rtv::ResponseState::Head(head) => {
///                 // the head contains headers etc.
///                 pritnln!("Content-Length: {}", head.content_length);
///                 pritnln!("Some header: {}", head.headers[0]);
///             },
///             rtv::ResponseState::Data(some_data) => {
///                 // you will receive data in small chunks as it comes in
///                 response_body.extend_from_slice(&some_data);
///             },
///             rtv::ResponseState::Done => {
///                 break 'ev;
///             },
///             other => panic!("Error: {}", other),
///         };
///     };
///     
///     events.clear();
///
/// }
///
/// let body_str = str::from_utf8(&response_body)?;
/// println!("{}", body_str);
///
/// ```
/// 
/// # Timeouts
///
/// You can set a timeout for every individual request that will even be
/// applied to dns resolution.
/// Remeber to pass the smallest timeout for any of the requests you sent into the
/// `mio::Poll::poll` function.
/// You need to do this because when a request times out no event is generated.
///
/// ```rust
/// client.send(&io, req1); // imagine 750ms timeout set on this request
/// client.send(&io, req2); // imagine 3s timeout set on this other one
/// io.poll(&mut events, Some(Duration::from_millis(750)))?; // poll with smallest timeout
/// ```
///
pub struct Client {
    dns: dns::DnsClient,
    dns_cache: HashMap<String, CachedAddr>,
    requests: Vec<InternalRequest>,
    next_id: usize,
    #[cfg(feature = "tls")]
    tls_config: Arc<rustls::ClientConfig>,
    #[cfg(not(feature = "tls"))]
    tls_config: (),
}

impl Client {

    /// Creates a new client.
    ///
    /// The token you pass in will be used for dns resolution as
    /// this requires (only) one socket.
    pub fn new(token: mio::Token) -> Self {

        let tls_config = Self::make_tls_config();
        
        Self {
            dns: dns::DnsClient::new(token),
            dns_cache: HashMap::new(),
            requests: Vec::new(),
            next_id: 0,
            tls_config,
        }

    }

    /// Send a request.
    ///
    /// The token you pass in will be used for this request's TCP connection.
    /// It will be available again once the request completed.
    ///
    /// This function will return a [`ReqId`] that can be used to check which response_body
    /// belongs to which request later.
    ///
    /// For more information on how to create a request see [`Request`] and [`RequestBuilder`](crate::RequestBuilder).
    /// If you wanna set a timeout, you can do that when creating a request.
    /// Currently `keep-alive` is **not** supported.
    /// 
    /// This function can take anything that implements `Into<Request>` so you can pass it a
    /// `Request` or a `RequestBuilder`, both will work.
    ///
    /// # Example
    ///
    /// ```rust
    /// let request = Request::get().host("example.com");
    /// client.send(&io, mio::Token(1), request)?; // io is the mio::Poll
    /// ```
    ///
    pub fn send(&mut self, io: &mio::Poll, token: mio::Token, input: impl Into<Request>) -> io::Result<ReqId> {

        let request = input.into();

        let request_bytes = request.format();

        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        let mode = InternalMode::from_mode(request.mode, &self.tls_config, request.uri.host.clone());

        let maybe_cached = self.dns_cache.get(&request.uri.host);
        let (connection, state) = match maybe_cached {

            Some(cached_addr) if !is_elapsed(cached_addr.time_created, Some(cached_addr.ttl)) => {

                let mut connection = Connection::new(cached_addr.ip_addr, mode)?;
                register_all(io, &mut connection, token)?;
                (Some(connection), InternalRequestState::Sending)

            },

            _not_cached_or_invalid => {

                let dns_id = self.dns.resolve(io, request.uri.host.clone(), request.timeout)?;
                (None, InternalRequestState::Resolving { id: dns_id, mode: Some(mode) })

            },

        };

        let internal_request = InternalRequest {
            host: request.uri.host,
            id,
            token,
            state,
            connection,
            request_bytes,
            current_result: Vec::new(),
            body_begin: 0,
            body_length: 0,
            body_bytes_read: 0,
            transfer_chunked: false,
            time_created: Instant::now(),
            timeout: request.timeout,
        };

        self.requests.push(internal_request);

        Ok(ReqId { inner: id })

    }

    /// Drive all sent requests to completion and get the responses.
    ///
    /// The `pump` function must be executed everytime an event is generated which
    /// belongs to this `Client`. You don't need to match against the event token
    /// yourself though as this is done internally.
    /// All events not belonging to this `Client` will be ignored.
    ///
    /// This function will return a `Vec` of responses, that contain the [`ReqId`] of
    /// the request that belongs to the response.
    /// The returned `Vec` may be empty, for example if the event belonged to dns resolution.
    ///
    /// In general a request will go through following stages:
    /// 1. Dns resolution, which will generate one or more events.
    /// 2. Receiving the head, with information about the response such as the content length
    ///    ([`ResponseState::Head`]).
    /// 3. Receiving the body, which will generate multiple events and responses
    ///    ([`ResponseState::Data`]).
    ///
    /// # Example
    ///
    /// ```rust
    /// let resps = client.pump(&io, &events)?;
    /// if resps.is_empty() { println!("Got an event but no response yet!") }
    /// for resp in resps {
    ///     println!("Got a response: {:?}", resp.state);
    /// }
    /// ```
    ///
    pub fn pump(&mut self, io: &mio::Poll, events: &mio::Events) -> io::Result<Vec<Response>> {

        let mut responses = Vec::new();

        let dns_resps = self.dns.pump(&io, events)?;

        let mut index: isize = 0;
        while let Some(request) = self.requests.get_mut(index as usize) {

            if is_elapsed(request.time_created, request.timeout) {

                let mut moved_request = self.requests.remove(index as usize);
                index -= 1;

                // there may be no stream since dns resolution might not be finished
                if let Some(mut stream) = moved_request.connection.take() {
                    io.registry().deregister(&mut stream)?;
                }

                responses.push(Response::new(moved_request.id, ResponseState::TimedOut))

            }

            index += 1;

        }

        for event in events {

            let mut index: isize = 0;
            while let Some(request) = self.requests.get_mut(index as usize) {

                if let Some(ref mut connection) = request.connection {
                    // we need to "pump" rustls so it can do the handshake etc.
                    connection.complete_io()?;
                }

                match request.state {

                    InternalRequestState::Resolving { id: dns_id, ref mut mode } => {

                        for resp in dns_resps.iter() {

                            if resp.id == dns_id {

                                let (ip_addr, ttl) = match resp.outcome {
                                    dns::DnsOutcome::Known { addr, ttl } => (addr, ttl),
                                    dns::DnsOutcome::Unknown => {
                                        responses.push(Response::new(request.id, ResponseState::UnknownHost));
                                        Self::finish_request(&io, &mut self.requests, &mut index)?;
                                        break;
                                    },
                                    dns::DnsOutcome::Error => {
                                        responses.push(Response::new(request.id, ResponseState::Error));
                                        Self::finish_request(&io, &mut self.requests, &mut index)?;
                                        break;
                                    },
                                    dns::DnsOutcome::TimedOut => {
                                        responses.push(Response::new(request.id, ResponseState::TimedOut));
                                        Self::finish_request(&io, &mut self.requests, &mut index)?;
                                        break;
                                    },
                                };

                                self.dns_cache.insert(request.host.clone(), CachedAddr {
                                    ip_addr,
                                    time_created: Instant::now(),
                                    ttl,
                                });

                                let mut connection = Connection::new(ip_addr, mode.take().expect("Mode was taken."))?;
                                register_all(io, &mut connection, request.token)?;

                                request.connection = Some(connection);
                                request.state = InternalRequestState::Sending;

                                break;

                            }
                            
                        }

                    },

                    InternalRequestState::Sending => {

                        if event.token() == request.token {

                            let connection = request.connection.as_mut().expect("No connection.");

                            match connection.peer_addr() {
                                Ok(..) => {

                                    match connection.write(&request.request_bytes) {
                                        Ok(..) => (),
                                        Err(err) if wouldblock(&err) => continue, // during tls handshake it blocks
                                        Err(other) => return Err(other),
                                    };

                                    request.state = InternalRequestState::RecvHead;

                                },
                                Err(err) if notconnected(&err) => continue,
                                Err(other) => return Err(other),
                            }

                        }

                    },

                    InternalRequestState::RecvHead => {

                        if event.token() == request.token {

                            // we will get another `writable` event after sending the payload
                            // so we have to check here that this is actually a `readable` event
                            if event.is_readable() {

                                let (_bytes_read, was_closed) = Self::client_read(request)?;

                                let mut has_valid_header = false;
                                if let Some((head, body_begin)) = ResponseHead::parse(&request.current_result) {

                                    has_valid_header = true;

                                    request.current_result.drain(..body_begin);

                                    request.body_bytes_read = request.current_result.len();

                                    request.body_begin = body_begin;
                                    request.body_length = head.content_length;
                                    request.transfer_chunked = head.transfer_chunked;

                                    request.state = InternalRequestState::RecvBody;

                                    responses.push(Response {
                                        id: ReqId { inner: request.id },
                                        state: ResponseState::Head(head)
                                    });

                                }

                                let is_done_with_body = has_valid_header && !request.transfer_chunked && request.body_bytes_read >= request.body_length;
                                let is_done_without_body = has_valid_header && !request.transfer_chunked && request.body_length == 0;
                                let is_done_or_closed = is_done_with_body | is_done_without_body | was_closed;

                                if is_done_or_closed {

                                    let moved_request = Self::finish_request(&io, &mut self.requests, &mut index)?;

                                    if is_done_with_body {
                                        responses.push(Response::new(moved_request.id, ResponseState::Data(moved_request.current_result)));
                                        responses.push(Response::new(moved_request.id, ResponseState::Done));
                                    } else if is_done_without_body {
                                        responses.push(Response::new(moved_request.id, ResponseState::Done));
                                    } else if was_closed {
                                        responses.push(Response::new(moved_request.id, ResponseState::Dead));
                                    } else {
                                        unreachable!()
                                    };

                                }

                            }

                        }

                    },

                    InternalRequestState::RecvBody => {

                        if event.token() == request.token {

                            // see above note
                            if event.is_readable() {

                                let (bytes_read, was_closed) = Self::client_read(request)?;

                                let mut data = Vec::new();
                                let mut is_done = false;

                                if request.transfer_chunked {

                                    // todo: if we recvHead and recv all chunked data immediatly we
                                    // will not process it (not like I care)

                                    // body_length and body_bytes_read is used in chunked transfer
                                    // mode to denote the current chunks length and how far we are into it

                                    // we loop because there might be multiple / incomplete chunks
                                    // in one packet
                                    loop {

                                        if request.body_bytes_read >= request.body_length {

                                            let head_end = request.current_result.windows(2).position(|bytes| bytes == &[0x0D, 0x0A] /* CRLF */).unwrap(); // todo
                                            let head_str = std::str::from_utf8(&request.current_result[..head_end]).expect("Chunk head is not valid Utf8.");
                                            let chunk_length: usize = usize::from_str_radix(head_str, 16).expect("Invalid chunk head / size number.");

                                            request.body_length = chunk_length;
                                            request.body_bytes_read = 0;
                                            request.current_result.drain(..head_end + 2 /* skip the two CLRF bytes */);

                                            if chunk_length == 0 {
                                                if cfg!(test) { assert!(&request.current_result[..2] == &[0x0D, 0x0A], "Expected CLRF.") }
                                                request.current_result.drain(..2); // remove the trailing (double) CLRF
                                                is_done = true;
                                                break;
                                            }

                                        }

                                        let bytes_just_read = request.current_result.len();
                                        let total_bytes_read = request.body_bytes_read + bytes_just_read;

                                        if request.body_length >= total_bytes_read {
                                            
                                            // not enough data
                                            data.extend(request.current_result.drain(..));
                                            request.body_bytes_read += bytes_just_read;
                                            break

                                        } else {

                                            // too much data or exactly enough
                                            data.extend(request.current_result.drain(..request.body_length - request.body_bytes_read));
                                            if cfg!(test) { assert!(&request.current_result[..2] == &[0x0D, 0x0A], "Expected CLRF.") }
                                            request.current_result.drain(..2); // remove the trailing CRLF
                                            request.body_bytes_read = request.body_length;

                                            // maybe we read exactly one chunk, in which case we
                                            // need to wait for more data to come
                                            if request.current_result.is_empty() {
                                                break
                                            }

                                        }

                                    }

                                    if cfg!(test) { assert!(request.current_result.len() == 0, "is {:?}", String::from_utf8_lossy(&request.current_result)) };

                                } else {

                                    data.append(&mut request.current_result);

                                    request.body_bytes_read += bytes_read;
                                    is_done = request.body_bytes_read >= request.body_length;

                                }


                                responses.push(Response {
                                    id: ReqId { inner: request.id },
                                    state: ResponseState::Data(data),
                                });

                                let is_done_or_closed = is_done | was_closed;

                                if is_done_or_closed {

                                    let moved_request = Self::finish_request(&io, &mut self.requests, &mut index)?;

                                    if is_done {
                                        responses.push(Response::new(moved_request.id, ResponseState::Done));
                                    } else if was_closed {
                                        responses.push(Response::new(moved_request.id, ResponseState::Dead));
                                    } else {
                                        unreachable!()
                                    }

                                }

                            }

                        }
                    }

                };

                index += 1;

            }

        }

        Ok(responses)

    }

    // fn abort_request<'d>(io: &'d mio::Poll, requests: &'d mut Vec<InternalRequest<'a>>, index: &'d mut isize) -> io::Result<ResponseState> {

    //     let _request = Self::finish_request(io, requests, index)?;
    //     Ok(ResponseState::Error)

    // }

    fn finish_request<'d>(io: &'d mio::Poll, requests: &'d mut Vec<InternalRequest>, index: &'d mut isize) -> io::Result<InternalRequest> {

        let mut request = requests.remove(*index as usize);
        let mut stream = request.connection.take().expect("No stream.");
        io.registry().deregister(&mut stream)?;
        *index -= 1;

        Ok(request)

    }

    fn client_read<'d>(request: &'d mut InternalRequest) -> io::Result<(usize, bool)> {

        let tcp_stream = request.connection.as_mut().expect("No connection.");

        let mut total_bytes_read = 0;
        let mut closed = false;
        loop {

            let mut buff = [0; 1024];
            let bytes_read = match tcp_stream.read(&mut buff) {
                Ok(num) => num,
                Err(err) if wouldblock(&err) => break,
                Err(other) => return Err(other),
            };

            if bytes_read > 0 {
                total_bytes_read += bytes_read;
                request.current_result.extend_from_slice(&buff[..bytes_read]);
            } else {
                closed = true;
                break

            }

        }

        Ok((total_bytes_read, closed))

    }

    #[cfg(feature = "tls")]
    fn make_tls_config() -> Arc<rustls::ClientConfig> {

        let mut root_store = rustls::RootCertStore::empty();
        root_store.add_server_trust_anchors(
            webpki_roots::TLS_SERVER_ROOTS.0.iter().map(|ta| rustls::OwnedTrustAnchor::from_subject_spki_name_constraints(ta.subject, ta.spki, ta.name_constraints))
        );

        let config = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        Arc::new(config)

    }

    #[cfg(not(feature = "tls"))]
    fn make_tls_config() -> () {
        ()
    }

}

struct InternalRequest {
    id: usize,
    token: mio::Token,
    host: String,
    request_bytes: Vec<u8>,
    state: InternalRequestState,
    connection: Option<Connection>,
    current_result: Vec<u8>,
    body_begin: usize,
    body_length: usize,
    body_bytes_read: usize,
    transfer_chunked: bool, // if `true` the message is handeled as chunked transfer encoding
    time_created: Instant,
    timeout: Option<Duration>,
}

enum InternalRequestState {
    Resolving { id: dns::DnsId, mode: Option<InternalMode> },
    Sending,
    RecvHead,
    RecvBody,
}

struct CachedAddr {
    pub(crate) ip_addr: Ipv4Addr,
    pub(crate) time_created: Instant,
    pub(crate) ttl: Duration,
}

enum InternalMode {
    Plain,
    #[cfg(feature = "tls")]
    Secure { tls_config: Arc<rustls::ClientConfig>, server_name: rustls::ServerName }
}

impl InternalMode {

    #[cfg(feature = "tls")]
    pub(crate) fn from_mode(mode: Mode, tls_config: &Arc<rustls::ClientConfig>, host: String) -> Self {
        match mode {
            Mode::Plain => Self::Plain,
            Mode::Secure => Self::Secure {
                tls_config: Arc::clone(tls_config),
                server_name: host.as_str().try_into().expect("Invalid host name.")
            },
        }
    }

    #[cfg(not(feature = "tls"))]
    pub(crate) fn from_mode(_mode: Mode, _tls_config: &(), _host: &str) -> Self {
        Self::Plain
    }

}

enum Connection {
    Plain { tcp_stream: TcpStream },
    #[cfg(feature = "tls")]
    Secure { stream: rustls::StreamOwned<rustls::ClientConnection, TcpStream> },
}

impl Connection {

    pub(crate) fn new(ip_addr: Ipv4Addr, mode: InternalMode) -> io::Result<Self> {

        match mode {
            InternalMode::Plain => {
                let tcp_stream = TcpStream::connect(make_socket_addr(ip_addr, 80))?;
                Ok(Self::Plain { tcp_stream })
            },
            #[cfg(feature = "tls")]
            InternalMode::Secure { tls_config, server_name } => {
                let tcp_stream = TcpStream::connect(make_socket_addr(ip_addr, 443))?;
                let tls_connection = rustls::ClientConnection::new(tls_config, server_name).expect("todo: 1");
                let stream = rustls::StreamOwned::new(tls_connection, tcp_stream);
                Ok(Self::Secure { stream })
            }
        }

    }

    pub(crate) fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.tcp_stream().peer_addr()
    }

    pub(crate) fn tcp_stream(&self) -> &TcpStream {
        match self {
            Self::Plain { tcp_stream } => tcp_stream,
            #[cfg(feature = "tls")]
            Self::Secure { stream } => &stream.sock,
        }
    }

    pub(crate) fn tcp_stream_mut(&mut self) -> &mut TcpStream {
        match self {
            Self::Plain { tcp_stream } => tcp_stream,
            #[cfg(feature = "tls")]
            Self::Secure { stream } => &mut stream.sock,
        }
    }

    pub(crate) fn complete_io(&mut self) -> io::Result<()> {

        #[cfg(feature = "tls")]
        if let Connection::Secure { stream } = self {
            match stream.conn.complete_io(&mut stream.sock) {
                Ok(..) => (),
                Err(err) if wouldblock(&err) => (),
                Err(other) => return Err(other),
            };
        }

        Ok(())

    }

}

impl mio::event::Source for Connection {
    fn register(&mut self, registry: &mio::Registry, token: mio::Token, interests: mio::Interest) -> io::Result<()> {
        self.tcp_stream_mut().register(registry, token, interests)
    }
    fn reregister(&mut self, registry: &mio::Registry, token: mio::Token, interests: mio::Interest) -> io::Result<()> {
        self.tcp_stream_mut().reregister(registry, token, interests)
    }
    fn deregister(&mut self, registry: &mio::Registry) -> io::Result<()> {
        self.tcp_stream_mut().deregister(registry)
    }
}

impl Read for Connection {

    fn read(&mut self, buff: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Plain  { tcp_stream } => tcp_stream.read(buff),
            #[cfg(feature = "tls")]
            Self::Secure { stream } => stream.read(buff)
        }
    }

}

impl Write for Connection {

    fn write(&mut self, buff: &[u8]) -> io::Result<usize> {
        match self {
            Self::Plain  { tcp_stream } => tcp_stream.write(buff),
            #[cfg(feature = "tls")]
            Self::Secure { stream } => stream.write(buff)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Plain  { tcp_stream } => tcp_stream.flush(),
            #[cfg(feature = "tls")]
            Self::Secure { stream } => stream.flush()
        }
    }

}

