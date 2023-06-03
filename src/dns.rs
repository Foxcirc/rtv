
use mio::{net::UdpSocket, Interest};
use std::{io, net::{SocketAddr, IpAddr, Ipv4Addr}, io::Write, iter::repeat, process::id};

const ME:  SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
const DNS: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 53);

pub(crate) struct DnsClient<'a> {
    socket: UdpSocket,
    connected: bool,
    write_outdated: bool,
    token: mio::Token,
    requests: Vec<InternalRequest<'a>>,
    next_id: u16,
}

impl<'a> DnsClient<'a> {

    pub(crate) fn new(token: mio::Token) -> io::Result<Self> {
        Ok(Self {
            socket: UdpSocket::bind(ME)?,
            connected: false,
            write_outdated: false,
            token,
            requests: Vec::new(),
            next_id: 0
        })
    }

    pub(crate) fn resolve(&mut self, io: &mio::Poll, req: impl Into<DnsRequest<'a>>) -> io::Result<Id> {

        if !self.connected {
            self.socket.connect(DNS)?;
            io.registry().register(&mut self.socket, self.token, Interest::READABLE | Interest::WRITABLE)?;
            self.connected = true;
        }
        
        if self.write_outdated {
            io.registry().reregister(&mut self.socket, self.token, Interest::READABLE | Interest::WRITABLE)?;
            self.write_outdated = false;
        }

        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.requests.push(InternalRequest { state: InternalRequestState::Pending, inner: req.into(), id });

        Ok(Id { inner: id })

    }

    pub(crate) fn pump(&mut self, event: &mio::event::Event) -> io::Result<Option<DnsResponse>> {

        if self.token == event.token() {

            if event.is_writable() {

                self.write_outdated = true;

                if let Some(req) = self.requests.iter_mut().find(|req| req.state == InternalRequestState::Pending) {
                    let bytes = req.parse_into_packet();
                    self.socket.send(&bytes)?;
                    req.state = InternalRequestState::Sent;
                    self.write_outdated = false;
                }

            }

            if event.is_readable() {

                let mut buff = [0; 1024];
                self.socket.recv(&mut buff)?;
                let resp = DnsResponse::parse_from_packet(&buff);

                if let Some(idx) = self.requests.iter().position(|req| req.id == resp.id.inner) {
                    self.requests.swap_remove(idx);
                    if self.requests.is_empty() {
                        self.socket = UdpSocket::bind(ME)?;
                        self.connected = false;
                    }
                }

                return Ok(Some(resp))
            }
            
        }

        Ok(None)

    }

}

pub(crate) struct Id {
    pub(crate) inner: u16,
}

pub(crate) struct DnsRequest<'a> {
    pub name: &'a str,
}

impl<'a> From<&'a str> for DnsRequest<'a> {
    fn from(name: &'a str) -> Self {
        Self { name }
    }
}

struct InternalRequest<'a> {
    state: InternalRequestState,
    inner: DnsRequest<'a>,
    id: u16,
}

#[derive(PartialEq)]
enum InternalRequestState {
    Pending,
    Sent,
}

impl<'a> InternalRequest<'a> {

    fn parse_into_packet(&self) -> Vec<u8> {

        let mut packet = dns_parser::Builder::new_query(self.id, true);
        packet.add_question(self.inner.name, false, dns_parser::QueryType::A, dns_parser::QueryClass::IN);

        packet.build().unwrap()

    }

}

pub(crate) struct DnsResponse {
    pub id: Id,
    pub addr: Ipv4Addr,
}

impl DnsResponse {

    fn parse_from_packet(buff: &[u8]) -> Self {

        let packet = dns_parser::Packet::parse(buff).unwrap();

        let addr = match packet.answers[0].data {
            dns_parser::RData::A(res) => res.0,
            _ => unreachable!(),
        };

        Self {
            id: Id { inner: packet.header.id },
            addr,
        }

    }

}

