
//! # Rtv
//!
//! Rtv is a simple, minimal dependency HTTP client that runs ontop of [`mio`](https://docs.rs/mio/0.8.8/mio).
//!
//! This library doesn't use rust `async`, however it still allows you to asyncronously
//! send requests using [`mio`](https://docs.rs/mio/0.8.8/mio). This approach a bit more verbose but allows for a simpler
//! architecture, not requiring 100 dependencies.
//!
//! Depending on how much flexibility you need you can use different things:
//! - A [`Client`] gives you full controll and is used with a [`mio::Poll`](https://docs.rs/mio/latest/mio/struct.Poll.html).
//! - A [`SimpleClient`] makes it simple to send one request, send many requests at the same time
//!   or stream a request.
//!

mod util;
mod http;
mod dns;
mod client;
mod simple;

#[cfg(test)]
mod test;

pub use {
    http::*,
    client::Client,
    simple::{SimpleClient, SimpleResponse},
};

