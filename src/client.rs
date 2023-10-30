
//! This module contains an HTTP [`Client`] that runs ontop of `mio`.

use mio::net::TcpStream;
use std::{io::{self, Write, Read}, time::{Duration, Instant}, collections::HashMap, net::{SocketAddr, Ipv4Addr}, mem::replace};
use crate::{dns, util::{make_socket_addr, notconnected, register_all, wouldblock, hash}, ResponseHead, Request, ReqId, Response, ResponseState, Mode, Status, OwnedHeader};

#[cfg(feature = "tls")]
use std::sync::Arc;

/// A flexible HTTP client.
///
/// Use the client if you wanna have controll over `mio` yourself.
/// You should look at the documentation of the individual methods for more info on
/// what exactly they do.
///
/// In general, you pass the client a handle to your `Poll` when you send a request.
/// Inside you `mio` event loop, when you get an event, you then call the [`Client::pump`] function,
/// which drives the request to completion.
///
/// # Example
///
/// This is more or less a full blown example on what it takes to correctly
/// send a request.
///
/// ```rust
///
/// let io = mio::Poll::new()?;
/// let mut client = rtv::Client::new(mio::Token(0));
///
/// let request = Request::get().host("example.com").https();
/// let _id = client.send(&io, mio::Token(2), request)?;
/// //  ^^^ the returned id can be used to check which response belongs to which request
/// //      although we are just sending one request here so this isn't needed
///
/// // we have to store the body ourselfes
/// let mut response_body = Vec::new();
///
/// 'ev: loop {
///     
///     // see note below on how to handle timeouts
///     io.poll(&mut events, client.timeout())?;
///     
///     // loop over all the responses we may have gotten
///     // you don't need to handle events generated by rtv in any other way
///     for resp in client.pump(&io, &events)? {
///         match resp.state {
///             rtv::ResponseState::Head(head) => {
///                 // the head contains headers etc.
///                 pritnln!("Content-Length: {}", head.content_length);
///                 pritnln!("Some header: {}", head.headers[0]);
///             },
///             rtv::ResponseState::Data(some_data) => {
///                 // you will receive data in small chunks as it comes in
///                 response_body.extend(some_data);
///             },
///             rtv::ResponseState::Done => {
///                 break 'ev;
///             },
///             // maybe a timeout or I/O error
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
/// Rtv supports a timeout for every individual request. It will even be
/// applied to dns resolution.
///
/// You have to specify this timeout in two places. First, when creating your
/// `Request` and then once again when waiting for events with `mio`.
///
/// The timeout used with `mio` always has to match the smallest time left
/// for any request currently in progress, so that the `Client` can terminate
/// the request if the timeout is reached.
///
/// You could do this manually but you should probably use [`Client::timeout`]
/// which does the calculation for you.
pub struct Client {
    dns: dns::DnsClient,
    dns_cache: HashMap<u64, CachedAddr>,
    requests: Vec<InternalReq>,
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
    #[inline(always)]
    pub fn new(token: mio::Token) -> Self {

        let tls_config = Self::default_tls_config();
        
        Self {
            dns: dns::DnsClient::new(token),
            dns_cache: HashMap::new(),
            requests: Vec::new(),
            next_id: 0,
            tls_config,
        }

    }

    /// Creates a new client with a custom [`ClientConfig`](rustls::ClientConfig).
    ///
    /// The token you pass in will be used for dns resolution as
    /// this requires (only) one socket.
    #[cfg(feature = "tls")]
    #[inline(always)]
    pub fn with_tls_config(token: mio::Token, tls_config: Arc<rustls::ClientConfig>) -> Self {
        
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
    /// This function will return a [`ReqId`] that can be used to check which response
    /// belongs to which request later.
    ///
    /// For more information on how to create a request see [`Request`] and [`RequestBuilder`](crate::RequestBuilder).
    /// If you wanna set a timeout, you can do so when creating a request.
    /// This function can take anything that implements `Into<Request>` so you can pass it a
    /// `Request` or a `RequestBuilder`, both will work.
    /// 
    /// # Example
    ///
    /// ```rust
    /// let request = Request::get().host("example.com");
    /// client.send(&io, mio::Token(1), request)?; // io is the mio::Poll
    /// ```
    pub fn send<'a>(&mut self, io: &mio::Poll, token: mio::Token, input: impl Into<Request<'a>>) -> io::Result<ReqId> {

        let request = input.into();

        let request_bytes = request.format();

        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        let mode = InternalMode::from_mode(request.mode, &self.tls_config, request.uri.host);

        let maybe_cached = self.dns_cache.get(&hash(request.uri.host));
        let state = match maybe_cached {

            Some(cached_addr) if !cached_addr.is_outdated() => {

                let mut connection = Connection::new(cached_addr.ip_addr, mode)?;
                register_all(io, &mut connection, token)?;
                InternalReqState::Sending {
                    body: request_bytes,
                    connection,
                }

            },

            _not_cached_or_old => {

                let dns_id = self.dns.resolve(io, request.uri.host, request.timeout)?;
                InternalReqState::Resolving {
                    body: request_bytes,
                    dns_id,
                    host: request.uri.host.to_string(), // todo: zero-alloc: hash the host and store the hash
                    mode
                }

            },

        };

        let internal_req = InternalReq {
            id,
            token,
            state,
            time_created: Instant::now(),
            timeout: request.timeout,
        };

        self.requests.push(internal_req);

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
    /// 2. Receiving the head, with information about the response such as the content length.
    ///    ([`ResponseState::Head`])
    /// 3. Receiving the body, which will generate multiple events.
    ///    ([`ResponseState::Data`])
    /// 4. In the end either [`ResponseState::Done`] or [`ResponseState::Error`].
    ///
    /// # Example
    ///
    /// ```rust
    /// let events = ...; // wait for new events using mio
    /// let resps = client.pump(&io, &events)?;
    /// if resps.is_empty() { println!("Got an event but no response yet!") }
    /// for resp in resps {
    ///     println!("Got a response: {:?}", resp.state);
    /// }
    /// ```
    pub fn pump(&mut self, io: &mio::Poll, events: &mio::Events) -> io::Result<Vec<Response>> {

        let mut responses = Vec::new();

        let dns_resps = self.dns.pump(&io, events)?;

        'rq: for request in self.requests.iter_mut() {

            // finish timed out requests
            if request.timeout.unwrap_or(Duration::MAX) <= request.time_created.elapsed() {

                responses.push(Response::new(request.id, ResponseState::TimedOut));
                request.deregister(&io)?; // todo: make io errors not "hard errors" but make them
                // also be per-request and make it so that you can retry completing the request
                // after an io error (maybe?)
                request.finish_error();

            } else {

                if let Some(connection) = request.state.connection_mut() {
                    // we need to "pump" rustls so it can do the handshake etc.
                    connection.complete_io()?;
                }

                for event in events.iter() {

                    match &mut request.state {

                        InternalReqState::Resolving { dns_id, .. } => {

                            for resp in dns_resps.iter() {

                                if &resp.id == dns_id {

                                    // dispatch the result
                                    // we don't need to call deregister on error since
                                    // we haven't registered anything yet
                                    let (addr, ttl) = match resp.outcome {
                                        dns::DnsOutcome::Known { addr, ttl } => (addr, ttl),
                                        dns::DnsOutcome::Unknown => {
                                            responses.push(Response::new(request.id, ResponseState::UnknownHost));
                                            request.finish_error();
                                            continue 'rq;
                                        },
                                        dns::DnsOutcome::Error => {
                                            responses.push(Response::new(request.id, ResponseState::Error));
                                            request.finish_error();
                                            continue 'rq;
                                        },
                                        dns::DnsOutcome::TimedOut => {
                                            responses.push(Response::new(request.id, ResponseState::TimedOut));
                                            request.finish_error();
                                            continue 'rq;
                                        },
                                    };

                                    let state = replace(&mut request.state, InternalReqState::Unspecified);
                                    if let InternalReqState::Resolving { body, host, mode, .. } = state {

                                        self.dns_cache.insert(hash(host), CachedAddr {
                                            ip_addr: addr,
                                            time_created: Instant::now(),
                                            ttl,
                                        });

                                        let mut connection = Connection::new(addr, mode)?;
                                        register_all(io, &mut connection, request.token)?;

                                        request.state = InternalReqState::Sending { body, connection };

                                        continue 'rq;

                                    } else {
                                        unreachable!()
                                    }

                                }
                                
                            }

                        },

                        InternalReqState::Sending { body, connection } => {

                            if event.token() == request.token {

                                match connection.peer_addr() {
                                    Ok(..) => {

                                        match connection.write(&body) {
                                            Ok(..) => (),
                                            // during tls handshake it blocks (since the stream is still in rustls's controll)
                                            Err(err) if wouldblock(&err) => continue 'rq,
                                            Err(other) => return Err(other),
                                        };

                                        let state = replace(&mut request.state, InternalReqState::Unspecified);
                                        if let InternalReqState::Sending { connection, .. } = state {

                                            request.state = InternalReqState::RecvHead {
                                                connection,
                                                buffer: Vec::with_capacity(1024),
                                            };

                                        } else {
                                            unreachable!()
                                        }

                                    },
                                    Err(err) if notconnected(&err) => continue 'rq,
                                    Err(other) => return Err(other),
                                }

                            }

                        },

                        // this is handeled in this kinda scuffed way to avoid some code duplication
                        // after succesfully reading the `Head` the state is updated to `RecvBody`
                        // which causes both the code for `RecvHead` and `RecvBody` to run
                        InternalReqState::RecvHead { .. } |
                        InternalReqState::RecvBody { .. } => {

                            if event.token() == request.token {

                                // we will get another `writable` event after sending the payload
                                // so we have to check here that this is actually a `readable` event
                                if event.is_readable() {

                                    if let InternalReqState::RecvHead { connection, buffer } = &mut request.state {

                                        let mut bytes_read = buffer.len();
                                        let mut closed = false; 

                                        loop {

                                            buffer.resize(bytes_read + 2048, 0u8);
                                            bytes_read += match connection.read(&mut buffer[bytes_read..]) {
                                                Ok(0) => { closed = true; break },
                                                Ok(num) => num,
                                                Err(err) if wouldblock(&err) => break,
                                                Err(other) => return Err(other),
                                            };

                                        }

                                        buffer.truncate(bytes_read);

                                        let mut headers = [httparse::EMPTY_HEADER; 1024]; // todo: make the max header count be controllable by the user
                                        let mut head = httparse::Response::new(&mut headers);
                                        let status = match head.parse(&buffer) {
                                            Ok(val) => val,
                                            Err(_err) => {
                                                responses.push(Response::new(request.id, ResponseState::Error));
                                                request.finish_error();
                                                continue 'rq;
                                            }
                                        };

                                        if let httparse::Status::Complete(body_start) = status {

                                            let content_length = head.headers.iter()
                                                .find(|header| header.name == "Content-Length")
                                                .map(|header| usize::from_str_radix(std::str::from_utf8(header.value)
                                                    .expect("Content-Length was invalid utf8"), 10)
                                                    .expect("Content-Length was not a number"))
                                                .unwrap_or_default();

                                            let transfer_chunked = head.headers.iter()
                                                .find(|header| header.name == "Transfer-Encoding" && header.value == b"chunked")
                                                .is_some();

                                            responses.push(Response {
                                                id: ReqId { inner: request.id },
                                                state: ResponseState::Head(ResponseHead {
                                                    status: Status {
                                                        code: head.code.expect("missing status code"),
                                                        reason: head.reason.expect("missing reason").to_string(),
                                                    },
                                                    content_length,
                                                    transfer_chunked,
                                                    headers: head.headers.iter().map(OwnedHeader::from).collect(),
                                                })
                                            });

                                            // remove the parsed head from the buffer
                                            buffer.drain(..body_start);

                                            let state = replace(&mut request.state, InternalReqState::Unspecified);
                                            if let InternalReqState::RecvHead { connection, buffer } = state {

                                                // println!("BUFFER:\n{}", String::from_utf8_lossy(&buffer));
                                                // todo: maybe use a bufReader at least for the chunked
                                                    // mode, since the Decoder calls .bytes() sometimes
                                                let chain = io::Cursor::new(buffer).chain(connection);
                                                let recv = if transfer_chunked {
                                                    RecvBody::Chunked(chunked_transfer::Decoder::new(chain))
                                                } else {
                                                    RecvBody::Plain(chain)
                                                };

                                                request.state = InternalReqState::RecvBody {
                                                    recv,
                                                    bytes_read_total: 0,
                                                    content_length
                                                };

                                                // fall through to RecvBody

                                            } else {
                                                unreachable!()
                                            }

                                        } else if closed {
                                            responses.push(Response::new(request.id, ResponseState::Error));
                                            request.finish_error();
                                            continue 'rq;
                                        }

                                    }

                                }

                                if let InternalReqState::RecvBody { recv, bytes_read_total, content_length } = &mut request.state {

                                    let mut data = Vec::new();
                                    let mut bytes_read = 0;
                                    let mut closed = false; 

                                    loop {

                                        data.resize(bytes_read + 2048, 0u8);
                                        bytes_read += match recv.read(&mut data[bytes_read..]) {
                                            Ok(0) => { closed = true; break },
                                            Ok(num) => num,
                                            Err(err) if wouldblock(&err) => break,
                                            Err(other) => return Err(other),
                                        };

                                    }

                                    data.truncate(bytes_read);

                                    if bytes_read > 0 {

                                        // return the data we just read as a response
                                        responses.push(Response {
                                            id: ReqId { inner: request.id },
                                            state: ResponseState::Data(data),
                                        });

                                        *bytes_read_total += bytes_read;

                                    }

                                    let is_chunked = recv.is_chunked();
                                    if  is_chunked && (closed == true) ||
                                       !is_chunked && (bytes_read_total >= content_length) {

                                        responses.push(Response {
                                            id: ReqId { inner: request.id },
                                            state: ResponseState::Done,
                                        });

                                        request.deregister(&io)?;
                                        request.finish_done();

                                        continue 'rq

                                    } else if closed {
                                        responses.push(Response::new(request.id, ResponseState::Error));
                                        request.finish_error();
                                        continue 'rq;
                                    }

                                }

                            }

                        },

                        _other => todo!(),

                    }
                    
                }

            }

        }

        // remove all the finished requests
        self.requests.retain(|request|
            !request.is_finished()
        );

        Ok(responses)

    }

    /// Returns the smallest timeout for any of the current requests.
    ///
    /// Use this function to always correctly set the timeout when waiting for events with `mio`.
    ///
    /// # Example
    ///
    /// ```rust
    /// client.send(&io, req1); // imagine 750ms timeout set on this request
    /// client.send(&io, req2); // imagine 3s timeout set on this other one
    /// io.poll(&mut events, client.timeout())?; // poll with smallest time left (here ~750ms)
    /// ```
    ///
    /// # Note
    ///
    /// This function comes with a very small runtime cost sinc it has to loop over all current requests.
    #[inline(always)]
    pub fn timeout(&self) -> Option<Duration> {
        let now = Instant::now();
        self.requests.iter().filter_map(|request|
            request.timeout.map(|timeout| timeout.checked_sub(now - request.time_created).unwrap_or(Duration::ZERO))
        ).min()
    }

    #[cfg(feature = "tls")]
    #[inline(always)]
    fn default_tls_config() -> Arc<rustls::ClientConfig> {

        let mut root_store = rustls::RootCertStore::empty();
        root_store.add_server_trust_anchors(
            webpki_roots::TLS_SERVER_ROOTS.0.iter().map(|ta|
                rustls::OwnedTrustAnchor::from_subject_spki_name_constraints(ta.subject, ta.spki, ta.name_constraints)
            )
        );

        let config = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        Arc::new(config)

    }

    #[cfg(not(feature = "tls"))]
    fn default_tls_config() -> () {
        ()
    }

}

struct InternalReq {
    id: usize,
    token: mio::Token,
    time_created: Instant,
    timeout: Option<Duration>,
    state: InternalReqState,
}

impl InternalReq {
    pub fn deregister(&mut self, io: &mio::Poll) -> io::Result<()> {
        if let Some(conn) = self.state.connection_mut() {
            io.registry().deregister(conn)
        } else {
            Ok(())
        }
    }
    pub fn finish_done(&mut self) {
        let _unused = replace(&mut self.state, InternalReqState::Done);
    }
    pub fn finish_error(&mut self) {
        let _unused = replace(&mut self.state, InternalReqState::Error);
    }
    pub fn is_finished(&self) -> bool {
        matches!(&self.state, InternalReqState::Done | InternalReqState::Error)
    }
}

enum InternalReqState {
    Unspecified,
    Error,
    Done,
    Resolving {
        body: Vec<u8>, // sent later
        dns_id: dns::DnsId,
        host: String, // used for caching
        mode: InternalMode, // used to create the connection later
    },
    Sending   {
        body: Vec<u8>, // sent during this state
        connection: Connection,
    },
    RecvHead  {
        connection: Connection,
        buffer: Vec<u8>,
    },
    RecvBody  {
        recv: RecvBody,
        bytes_read_total: usize,
        content_length: usize,
    },
}

impl InternalReqState {
    pub fn connection_mut(&mut self) -> Option<&mut Connection> {
        match self {
            Self::Sending { connection, .. } => Some(connection),
            Self::RecvHead { connection, .. } => Some(connection),
            Self::RecvBody { recv, .. } => Some(recv.connection_mut()),
            _other => None,
        }
    }
}

enum RecvBody {
    Plain(io::Chain<io::Cursor<Vec<u8>>, Connection>),
    Chunked(chunked_transfer::Decoder<io::Chain<io::Cursor<Vec<u8>>, Connection>>)
}

impl RecvBody {
    pub fn connection_mut(&mut self) -> &mut Connection {
        match self {
            Self::Plain(conn) => conn.get_mut().1,
            Self::Chunked(decoder) => decoder.get_mut().get_mut().1
        }
    }
    pub fn is_chunked(&self) -> bool {
        match self {
            Self::Plain(..) => false,
            Self::Chunked(..) => true
        }
    }
}

impl io::Read for RecvBody {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Plain(conn) => conn.read(buf),
            Self::Chunked(decoder) => decoder.read(buf)
        }
    }
}

struct CachedAddr {
    pub ip_addr: Ipv4Addr,
    pub time_created: Instant,
    pub ttl: Duration,
}

impl CachedAddr {
    pub fn is_outdated(&self) -> bool {
        self.ttl <= self.time_created.elapsed()
    }
}

enum InternalMode {
    Plain,
    #[cfg(feature = "tls")]
    Secure { tls_config: Arc<rustls::ClientConfig>, server_name: rustls::ServerName }
}

impl InternalMode {

    #[cfg(feature = "tls")]
    pub(crate) fn from_mode(mode: Mode, tls_config: &Arc<rustls::ClientConfig>, host: &str) -> Self {
        match mode {
            Mode::Plain => Self::Plain,
            Mode::Secure => Self::Secure {
                tls_config: Arc::clone(tls_config),
                server_name: host.try_into().expect("invalid host name")
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
                let tls_connection = rustls::ClientConnection::new(tls_config, server_name).map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
                let stream = rustls::StreamOwned::new(tls_connection, tcp_stream);
                Ok(Self::Secure { stream })
            }
        }

    }

    pub(crate) fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.tcp_stream().peer_addr()
    }

    fn tcp_stream(&self) -> &TcpStream {
        match self {
            Self::Plain { tcp_stream } => tcp_stream,
            #[cfg(feature = "tls")]
            Self::Secure { stream } => &stream.sock,
        }
    }

    fn tcp_stream_mut(&mut self) -> &mut TcpStream {
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

