
use mio::net::UdpSocket;
use std::{io, net::{SocketAddr, Ipv4Addr}, fmt, time::{self, Duration, Instant}};
use crate::util::{make_socket_addr, register_all, wouldblock, reregister_all, is_elapsed};

const ME:  SocketAddr = make_socket_addr(Ipv4Addr::new(0, 0, 0, 0), 0);
const DNS: SocketAddr = make_socket_addr(Ipv4Addr::new(8, 8, 8, 8), 53); // google dns server
// todo: make custom dns server possible
// todo: clean this up and loom over the code again
// todo: remove the dependency on "dns_parser"

pub(crate) struct DnsClient {
    pub(crate) token: mio::Token,
    socket: Option<UdpSocket>,
    write_outdated: bool,
    requests: Vec<InternalRequest>,
    next_id: u16,
}

impl DnsClient {

    pub(crate) fn new(token: mio::Token) -> Self {
        Self {
            socket: None,
            write_outdated: false,
            token,
            requests: Vec::new(),
            next_id: 0
        }
    }

    pub(crate) fn resolve(&mut self, io: &mio::Poll, host: impl Into<String>, timeout: Option<Duration>) -> io::Result<DnsId> {

        if self.socket.is_none() {
            let mut socket = UdpSocket::bind(ME)?;
            socket.connect(DNS)?;
            register_all(io, &mut socket, self.token)?;
            self.socket = Some(socket);
        }
        
        if self.write_outdated {
            let socket = self.socket.as_mut().expect("No socket.");
            reregister_all(io, socket, self.token)?;
            self.write_outdated = false;
        }

        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        self.requests.push(InternalRequest {
            id,
            state: InternalRequestState::Pending,
            host: host.into(),
            time_created: Instant::now(),
            timeout,
        });

        Ok(DnsId { inner: id })

    }

    pub(crate) fn pump(&mut self, io: &mio::Poll, events: &mio::Events) -> io::Result<Vec<DnsResponse>> {

        let mut responses = Vec::new();

        let mut index: isize = 0;
        while let Some(request) = self.requests.get_mut(index as usize) {

            if is_elapsed(request.time_created, request.timeout) {

                let id = request.id;

                self.requests.swap_remove(index as usize);
                index -= 1;

                responses.push(DnsResponse {
                    id: DnsId { inner: id },
                    outcome: DnsOutcome::TimedOut
                })

            }

            index += 1;

        }

        for event in events {

            if self.token == event.token() {

                // we get another `writable` event after reading the
                // last response, so there may not be a socket even if we get an event
                if let Some(ref mut socket) = self.socket {

                    if event.is_writable() {

                        self.write_outdated = true;
                        for req in self.requests.iter_mut() {

                            if req.state == InternalRequestState::Pending {

                                let bytes = req.parse_into_packet();
                                socket.send(&bytes)?;

                                req.state = InternalRequestState::Sent;
                                self.write_outdated = false;

                            }

                        }

                    }

                    if event.is_readable() {

                        loop {

                            let mut buff = [0; 1024];

                            match socket.recv(&mut buff) {
                                Err(err) if wouldblock(&err) => break,
                                Err(other) => return Err(other),
                                Ok(..) => (),
                            };

                            let resp = DnsResponse::parse_from_packet(&buff);

                            // the request might have timeout out and thus be removed earlier
                            let maybe_idx = self.requests.iter().position(|req| req.id == resp.id.inner);
                            if let Some(idx) = maybe_idx {

                                responses.push(resp);

                                self.requests.swap_remove(idx);

                                if self.requests.is_empty() {
                                    io.registry().deregister(socket)?;
                                    self.socket = None;
                                    break
                                }

                            }

                        }

                    }

                }
                
            }

        }

        Ok(responses)

    }

}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct DnsId {
    pub(crate) inner: u16,
}

struct InternalRequest {
    id: u16,
    state: InternalRequestState,
    host: String,
    time_created: Instant,
    timeout: Option<Duration>,
}

#[derive(PartialEq)]
enum InternalRequestState {
    Pending,
    Sent,
}

impl InternalRequest {

    fn parse_into_packet(&self) -> Vec<u8> {

        let mut packet = dns_parser::Builder::new_query(self.id, true);
        packet.add_question(&self.host, false, dns_parser::QueryType::A, dns_parser::QueryClass::IN);

        packet.build().unwrap()

    }

}

#[derive(Debug)]
pub(crate) enum DnsOutcome {
    Known { addr: Ipv4Addr, ttl: time::Duration },
    Unknown,
    Error,
    TimedOut,
}

pub(crate) struct DnsResponse {
    pub(crate) id: DnsId,
    pub(crate) outcome: DnsOutcome,
}

impl DnsResponse {

    fn parse_from_packet(buff: &[u8]) -> Self {

        let packet = dns_parser::Packet::parse(buff).unwrap();

        let outcome = match packet.header.response_code {
            dns_parser::ResponseCode::NoError => {
                match parse_answer(&packet) {
                    Some((addr, ttl)) => DnsOutcome::Known { addr, ttl },
                    None => DnsOutcome::Error,
                }
            },
            dns_parser::ResponseCode::NameError => {
                DnsOutcome::Unknown
            },
            _ => {
                DnsOutcome::Error
            }
        };

        Self { id: DnsId { inner: packet.header.id }, outcome }

    }

}

fn parse_answer(packet: &dns_parser::Packet) -> Option<(Ipv4Addr, time::Duration)> {
    for answer in &packet.answers {
        if let dns_parser::RData::A(result) = answer.data {
            return Some((result.0, time::Duration::from_secs(answer.ttl as u64)))
        }
    }
    None
}

impl fmt::Debug for DnsResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.outcome {
            DnsOutcome::Known { addr, ttl } => write!(f, "{:?}, ttl: {:?}", addr, ttl),
            DnsOutcome::Unknown => write!(f, "Unknown"),
            DnsOutcome::Error => write!(f, "Error"),
            DnsOutcome::TimedOut => write!(f, "TimedOut"),
        }
    }
}

