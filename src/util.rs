
use mio::{event::Source, Interest};
use std::{net::{Ipv4Addr, SocketAddr, SocketAddrV4}, io, hash::{Hasher, Hash}, collections::hash_map::DefaultHasher};

pub(crate) fn hash(input: impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    hasher.finish()
}

pub fn wouldblock(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::WouldBlock
}

pub(crate) fn notconnected(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::NotConnected
}

pub(crate) const fn make_socket_addr(ip_addr: Ipv4Addr, port: u16) -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(ip_addr, port))
}

pub(crate) fn register_all<S: Source>(io: &mio::Poll, source: &mut S, token: mio::Token) -> io::Result<()> {
    io.registry().register(source, token, Interest::READABLE | Interest::WRITABLE)
}

pub(crate) fn reregister_all<S: Source>(io: &mio::Poll, source: &mut S, token: mio::Token) -> io::Result<()> {
    io.registry().reregister(source, token, Interest::READABLE | Interest::WRITABLE)
}

