
use std::{io::{self, Write, Read}, time::{Duration, Instant}, collections::HashMap, net::SocketAddr};
use mio::net::TcpStream;
use crate::{dns, util::{new_sock_addr, notconnected, register_all, wouldblock, is_elapsed}, ResponseHead, Request, ReqId, Response, ResponseState};

pub struct Client<'a> {
    dns: dns::DnsClient<'a>,
    dns_cache: HashMap<&'a str, Connection>,
    requests: Vec<InternalRequest<'a>>,
    next_id: usize,
}

impl<'a> Client<'a> {

    pub fn new(token: mio::Token) -> Self {
        Self {
            dns: dns::DnsClient::new(token),
            dns_cache: HashMap::new(),
            requests: Vec::new(),
            next_id: 0,
        }
    }

    pub fn send(&mut self, io: &mio::Poll, token: mio::Token, input: impl Into<Request<'a>>) -> io::Result<ReqId> {

        let request = input.into();

        let request_bytes = request.format();

        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        let maybe_conn = self.dns_cache.get(request.uri.host);

        let (stream, state) = match maybe_conn {

            Some(conn) if !is_elapsed(conn.time_created, Some(conn.ttl)) => {

                let mut tcp_stream = TcpStream::connect(conn.sock_addr)?;
                register_all(io, &mut tcp_stream, token)?;

                (Some(tcp_stream), InternalRequestState::Sending)

            },

            _not_cached => {

                let dns_id = self.dns.resolve(io, request.uri.host, request.timeout)?;

                (None, InternalRequestState::Resolving(dns_id))

            },

        };

        let request = InternalRequest {
            host: request.uri.host,
            id,
            token,
            state,
            stream,
            request_bytes,
            current_result: Vec::new(),
            body_begin: 0,
            body_length: 0,
            body_bytes_read: 0,
            time_created: Instant::now(),
            timeout: request.timeout,
        };

        self.requests.push(request);

        Ok(ReqId { inner: id })

    }

    pub fn pump(&mut self, io: &mio::Poll, events: &mio::Events) -> io::Result<Vec<Response>> {

        let mut responses = Vec::new();

        let dns_resps = self.dns.pump(&io, events)?;

        let mut index: isize = 0;
        while let Some(request) = self.requests.get_mut(index as usize) {

            if is_elapsed(request.time_created, request.timeout) {

                let mut moved_request = self.requests.remove(index as usize);
                index -= 1;

                // there may be no stream since dns resolution might not be finished
                if let Some(mut stream) = moved_request.stream.take() {
                    io.registry().deregister(&mut stream)?;
                }

                responses.push(Response::new(moved_request.id, ResponseState::TimedOut))

            }

            index += 1;

        }

        for event in events {

            let mut index: isize = 0;
            while let Some(request) = self.requests.get_mut(index as usize) {

                match request.state {

                    InternalRequestState::Resolving(dns_id) => {

                        for resp in dns_resps.iter() {

                            if resp.id == dns_id {

                                let (ip_addr, ttl) = match resp.outcome {
                                    dns::DnsOutcome::Known { addr, ttl } => (addr, ttl),
                                    dns::DnsOutcome::Unknown => todo!("unknown host"),
                                    dns::DnsOutcome::Error => todo!("error resolving ip address"),
                                    dns::DnsOutcome::TimedOut => todo!("dns timed out"),
                                };

                                let sock_addr = new_sock_addr(ip_addr, 80);

                                self.dns_cache.insert(request.host, Connection {
                                    sock_addr,
                                    time_created: Instant::now(),
                                    ttl,
                                });

                                let mut tcp_stream = TcpStream::connect(sock_addr)?;
                                register_all(io, &mut tcp_stream, request.token)?;

                                request.stream = Some(tcp_stream);
                                request.state = InternalRequestState::Sending;

                            }
                            
                        }

                    },

                    InternalRequestState::Sending => {

                        if event.token() == request.token {

                            assert!(event.is_writable());

                            let tcp_stream = request.stream.as_mut().expect("No tcp stream.");

                            match tcp_stream.peer_addr() {

                                Ok(..) => {

                                    tcp_stream.write_all(&request.request_bytes)?;
                                    request.state = InternalRequestState::RecvHead;

                                },

                                Err(err) if notconnected(&err) => {
                                    return Ok(responses);
                                },

                                Err(other) => return Err(other),

                            }

                        }

                    },

                    InternalRequestState::RecvHead => {

                        if event.token() == request.token {

                            // we will get another `writable` event after sending the payload
                            // so we have to check here that this is actually a `readable` event
                            if event.is_readable() {

                                let (_bytes_read, was_closed) = Self::tcp_read(request)?;

                                let mut has_valid_header = false;
                                if let Some((head, body_begin)) = ResponseHead::parse(&request.current_result) {

                                    has_valid_header = true;

                                    request.body_begin = body_begin;
                                    request.body_length = head.content_length;
                                    request.current_result.drain(..body_begin);
                                    request.body_bytes_read = request.current_result.len();
                                    request.state = InternalRequestState::RecvBody;

                                    responses.push(Response {
                                        id: ReqId { inner: request.id },
                                        state: ResponseState::Head(head)
                                    });

                                }

                                let is_done_with_body = has_valid_header && request.body_bytes_read >= request.body_length;
                                let is_done_without_body = has_valid_header && request.body_length == 0;
                                let is_done_or_closed = is_done_with_body | is_done_without_body | was_closed;

                                if is_done_or_closed {

                                    let mut moved_request = self.requests.remove(index as usize);
                                    index -= 1;
                                    let mut stream = moved_request.stream.take().expect("No tcp stream.");
                                    io.registry().deregister(&mut stream)?;

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

                                let (bytes_read, was_closed) = Self::tcp_read(request)?;
                                request.body_bytes_read += bytes_read;

                                responses.push(Response {
                                    id: ReqId { inner: request.id },
                                    state: ResponseState::Data(request.current_result.drain(..).collect()),
                                });

                                request.current_result = Vec::with_capacity(256);

                                let is_done = request.body_bytes_read >= request.body_length;
                                let is_done_or_closed = is_done | was_closed;

                                if is_done_or_closed {

                                    let mut moved_request = self.requests.remove(index as usize);
                                    index -= 1;
                                    let mut stream = moved_request.stream.take().expect("No tcp stream.");
                                    io.registry().deregister(&mut stream)?;

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

    fn tcp_read<'d>(request: &'d mut InternalRequest) -> io::Result<(usize, bool)> {

        let tcp_stream = request.stream.as_mut().expect("No tcp stream.");

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

}

struct InternalRequest<'a> {
    id: usize,
    token: mio::Token,
    host: &'a str,
    request_bytes: Vec<u8>,
    state: InternalRequestState,
    stream: Option<TcpStream>,
    current_result: Vec<u8>,
    body_begin: usize,
    body_length: usize,
    body_bytes_read: usize,
    time_created: Instant,
    timeout: Option<Duration>,
}

enum InternalRequestState {
    Resolving(dns::DnsId),
    Sending,
    RecvHead,
    RecvBody,
}

struct Connection {
    pub(crate) sock_addr: SocketAddr,
    pub(crate) time_created: Instant,
    pub(crate) ttl: Duration,
}

