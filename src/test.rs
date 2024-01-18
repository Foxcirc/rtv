
use std::{iter::once, time::Duration, array};
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
fn mio_http_request() {

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

    extreme::run(async {
        
        let mut client = SimpleClient::new().unwrap();

        let resp = client.send(Request::get().host("google.com")).await.unwrap();

        println!("Got a response!");
        println!("Body length: {}", resp.body.len());

    })

}

#[test]
fn chunked_request() {

    extreme::run(async {
        
        let mut client = SimpleClient::new().unwrap();

        let resp = client.send(Request::get().host("www.google.com")).await.unwrap();

        println!("Got a response!");

        let transfer_encoding = resp.head.get_header("Transfer-Encoding").unwrap();
        println!("Transfer-Encoding: {}", transfer_encoding);
        assert!(transfer_encoding == "chunked");

        println!("Head:");
        println!("{:?}", resp.head);

        // println!("{}", String::from_utf8_lossy(&resp.body));

    })


}

#[test]
fn many_request() {

    extreme::run(async {

        const NUM_REQUESTS: usize = 16;

        let mut client = SimpleClient::new().unwrap();

        let req1 = Request::get().host("google.com");
        let req2 = Request::get().host("example.com");
        let array: [_; NUM_REQUESTS] = array::from_fn(|_| req1.clone());
        let iter = array.into_iter().chain(once(req2));

        let mut futs = Vec::new();
        for req in iter {
            let fut = client.send(req);
            futs.push(fut);
        }

        // luckily rtv will egerly execute and push the requests to completion,
        // so we don't need join_all or an executor here ;)
        let mut resps = Vec::new();
        for fut in futs {
            let resp = fut.await;
            resps.push(resp);
        }

        println!("Total requests: {}", NUM_REQUESTS + 1);
        println!("Total responses: {}", resps.len());

        for result in resps.into_iter() {
            let resp = result.unwrap();
            println!("Got a response {:?}", resp);
        }
        
    })

}


#[test]
#[cfg(feature = "tls")]
fn streaming_request() {
    use futures_lite::AsyncReadExt;


    extreme::run(async {

        let mut client = SimpleClient::new().unwrap();

        let mut resp = client.stream(Request::get().secure().host("crates.io").user_agent("foxcirc's rtv")).await.unwrap();
        // println!("{:?}", resp.head);

        let mut buff = Vec::new();
        resp.body.read_to_end(&mut buff).await.unwrap();

        // println!("{}", buff);

        println!("Expected length: {}", resp.head.content_length);
        println!("Actual length: {}", buff.len());
        println!("Status: {:?}", resp.head.status);

        // assert!(resp.head.content_length == buff.len());
        
    })

}

#[test]
#[cfg(feature = "tls")]
fn secure_request() {

    extreme::run(async {

        let mut client = SimpleClient::new().unwrap();

        let _resp = client.send(Request::get().secure().host("www.wikipedia.org")).await.unwrap();

        // println!("resp: {}", resp.into_string_lossy());
        
    })

}

