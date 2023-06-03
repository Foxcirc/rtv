use crate::dns;


#[test]
fn dns_resolve() {

    let mut io = mio::Poll::new().unwrap();
    let mut events = mio::Events::with_capacity(16);

    let mut client = dns::DnsClient::new(mio::Token(0)).unwrap();

    client.resolve(&io, "google.com").unwrap();
    client.resolve(&io, "example.com").unwrap();
    client.resolve(&io, "discord.com").unwrap();
    client.resolve(&io, "youtube.com").unwrap();

    'ev: loop {

        io.poll(&mut events, None).unwrap();

        for event in events.iter() {

            println!("-> readable: {}, writable: {}", event.is_readable(), event.is_writable());
            if let Some(resp) = client.pump(&event).unwrap() {
                println!("Got adress: {:?}", resp.addr);
                // break 'ev;
            }

        }

        events.clear();

    }

}

