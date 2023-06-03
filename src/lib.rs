
use std::iter::once;

mod dns;

#[cfg(test)]
mod test;

pub struct Client {
    io: mio::Poll,
}

impl Client {

    pub fn new() -> Result<Self, std::io::Error> {
        mio::Poll::new().map(|io| Self { io }) // GG
    }

    pub fn send<B: ToString>(&self, request: Request<B>) -> Transmission {

        let request_text = request.http_format();

        todo!();

    }

}

pub struct Transmission {

}

pub struct Request<'a, B> {
    pub method: Method,
    pub uri: Uri<'a>,
    pub headers: Vec<Header<'a>>,
    pub body: B,
}

pub enum Method {
    Get,
}

pub struct Uri<'a> {
    pub host: &'a str,
    pub path: &'a str,
}

pub struct Header<'a> {
    pub name: &'a str,
    pub value: &'a str,
}

trait HttpFormat {
    fn http_format(&self) -> String; // todo: make it bytes or smth
}

impl HttpFormat for Method {
    fn http_format(&self) -> String {
        match self {
            Method::Get => "GET".to_string(),
        }
    }
}

impl<'a> HttpFormat for Vec<Header<'a>> {
    fn http_format(&self) -> String {
        self.into_iter().map(|header| header.http_format()).collect()
    }
}

impl<'a> HttpFormat for Header<'a> {
    fn http_format(&self) -> String {
        format!("{}: {}\n", self.name, self.value)
    }
}

impl<'a, B: ToString> HttpFormat for Request<'a, B> {
    fn http_format(&self) -> String {
        format!("{} {} HTTP/1.1\nHost: {}\n{}\n{}", self.method.http_format(), self.uri.path, self.uri.host, self.headers.http_format(), self.body.to_string())
    }
}

