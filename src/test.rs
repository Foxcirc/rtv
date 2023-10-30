
use std::{time::Duration, io::Read, fs};

use crate::{dns, Client, Request, SimpleClient};

#[test]
fn dns_resolve() {

    let mut io = mio::Poll::new().unwrap();
    let mut events = mio::Events::with_capacity(16);

    let mut client = dns::DnsClient::new(mio::Token(0));

    client.resolve(&io, "www.ionos.co.uk", None).unwrap();
    client.resolve(&io, "example.com", None).unwrap();
    client.resolve(&io, "discord.com", None).unwrap();
    client.resolve(&io, "youtube.com", None).unwrap();

    let mut counter = 0;
    'ev: loop {

        io.poll(&mut events, None).unwrap();

        for resp in client.pump(&io, &events).unwrap() {
            println!("Got an address: {:?}", resp);
            counter += 1;
            if counter == 4 { break 'ev }
        };

        events.clear();

    }

}

#[test]
fn request_builder() {

    Request::build()
        .https()
        .method(crate::Method::Get)
        .host("example.com")
        .path("/foo")
        .query("bar", "baz")
        .header("Timeout", "infinite")
        .user_agent("foxcirc's rtv")
        .send("send &str")
        .send(b"send &[u8]")
        .send(&[0, 1, 2, 3])
        .finish();

}

#[test]
fn http_request() {

    let mut io = mio::Poll::new().unwrap();
    let mut events = mio::Events::with_capacity(16);

    let mut client = Client::new(mio::Token(0));

    let req = Request::get()
        .secure()
        .timeout(Duration::from_secs(5))
        .host("www.google.com")
        .user_agent("foxcirc's rtv");

    let _id = client.send(&io, mio::Token(1), req).unwrap();

    'ev: loop {

        eprintln!("[polling]");
        io.poll(&mut events, client.timeout()).unwrap();
        eprintln!("[got {} events]", events.iter().count());

        for resp in client.pump(&io, &events).unwrap() {
            if let crate::ResponseState::Data(_data) = &resp.state {
                eprintln!("[got some data]");
                // eprintln!("[got some data, writing it to stdout]");
                // std::io::Write::write_all(&mut std::io::stdout(), _data).unwrap();
            } else {
                eprintln!("[got response state: {:?}]", resp.state);
            }
            if resp.state.is_finished() {
                break 'ev
            }
        };

        events.clear();

    }
    
}

#[test]
fn simple_request() {

    let mut client = SimpleClient::new().unwrap();

    let resp = client.send(Request::get().host("google.com")).unwrap();

    println!("Got a response!");
    println!("Body length: {}", resp.body.len());

}

#[test]
fn chunked_request() {

    let mut client = SimpleClient::new().unwrap();

    let resp = client.send(Request::get().host("www.google.com")).unwrap();

    println!("Got a response!");

    let transfer_encoding = resp.head.get_header("Transfer-Encoding").unwrap();
    println!("Transfer-Encoding: {}", transfer_encoding);
    assert!(transfer_encoding == "chunked");

    println!("Head:");
    println!("{:?}", resp.head);

    // println!("{}", String::from_utf8_lossy(&resp.body));

}

#[test]
fn many_request() {

    const NUM_REQUESTS: usize = 16;

    let mut client = SimpleClient::new().unwrap();

    let req = Request::get().host("google.com");
    let other_req = Request::get().host("example.com");
    let mut reqs = vec![req; NUM_REQUESTS];
    reqs.push(other_req);

    let resps = client.many(reqs).unwrap(); // todo: make SimpleClient::many support slices and not only Vec

    println!("Total requests: {}", NUM_REQUESTS + 1);
    println!("Total responses: {}", resps.len());

    for (idx, result) in resps.into_iter().enumerate() {
        let resp = result.unwrap();
        println!("Got a response {:?}", resp);
        if idx == NUM_REQUESTS + 1 - 1 {
            assert!(resp.body.len() == 1256, "Last resp must be the example.com one!");
            assert!(resp.head.status.code == 200, "Last resp must be the example.com one!");
        }
    }

}


#[test]
#[cfg(feature = "tls")]
fn streaming_request() {

    let mut client = SimpleClient::new().unwrap();

    let mut resp = client.stream(Request::get().secure().host("www.youtube.com")).unwrap();
    // println!("{:?}", resp.head);

    let mut buff = Vec::new();
    resp.body.read_to_end(&mut buff).unwrap();

    // println!("{}", buff);

    println!("Expected length: {}", resp.head.content_length);
    println!("Actual length: {}", buff.len());
    println!("Status: {:?}", resp.head.status);

    // assert!(resp.head.content_length == buff.len());

}

#[test]
#[cfg(feature = "tls")]
fn secure_request() {

    let mut client = SimpleClient::new().unwrap();

    let _resp = client.send(Request::get().secure().host("www.wikipedia.org")).unwrap();

    // println!("resp: {}", resp.into_string_lossy());

}

