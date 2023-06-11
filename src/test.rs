
use std::{time::Duration, io::Read};

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
fn http_request() {

    let mut io = mio::Poll::new().unwrap();
    let mut events = mio::Events::with_capacity(16);

    let mut client = Client::new(mio::Token(0));

    // let req = Request::get()
    //     .uri("google.com", "")
    //     .send_str("Hello world!");

    let req = Request {
        timeout: None,
        method: crate::Method::Get,
        mode: crate::Mode::Plain,
        uri: crate::Uri { host: "google.com", path: "" },
        headers: Vec::new(),
        body: b"",
    };

    let _id = client.send(&io, mio::Token(1), req).unwrap();

    'ev: loop {

        io.poll(&mut events, Some(Duration::from_secs(5))).unwrap();

        for resp in client.pump(&io, &events).unwrap() {
            println!("{:?}", resp.state);
            if resp.state == crate::ResponseState::Done {
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

    let te = resp.head.get_header("Transfer-Encoding").unwrap();

    println!("Got a response!");
    println!("Transfer-Encoding: {}", te);

    let body_str = String::from_utf8_lossy(&resp.body);
    // println!("{}", body_str);
    println!("Length = {}", body_str.len());

}

#[test]
fn many_request() {

    const NUM_REQUESTS: usize = 16;

    let mut client = SimpleClient::new().unwrap();

    let req = Request::get().host("google.com");
    let other_req = Request::get().host("example.com");
    let mut reqs = vec![req; NUM_REQUESTS];
    reqs.push(other_req);

    let resps = client.many(reqs).unwrap();

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
fn streaming_request() {

    let mut client = SimpleClient::new().unwrap();

    let mut resp = client.stream(Request::get().host("httpbin.org")).unwrap();

    println!("Expected length: {}", resp.head.content_length);
    let mut buff = Vec::new();
    resp.body.read_to_end(&mut buff).unwrap();
    println!("Actual length: {}", buff.len());
    assert!(resp.head.content_length == buff.len());

}

#[test]
fn secure_request() {

    let mut client = SimpleClient::new().unwrap();

    let resp = client.send(Request::get().secure().host("www.wikipedia.org")).unwrap();

    println!("done");
    // println!("resp: {}", resp.into_string_lossy());

}

