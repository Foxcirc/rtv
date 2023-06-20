
//! # Rtv
//!
//! Rtv is a simple, minimal dependency HTTP client that runs ontop of [`mio`](https://docs.rs/mio/0.8.8/mio).
//!
//! This library doesn't use rust `async`, however it still allows you to asyncronously
//! send requests using [`mio`](https://docs.rs/mio/0.8.8/mio). This approach a bit more verbose but allows for a simpler
//! architecture, not requiring 100 dependencies.
//! 
//! Depending on how much flexibility you need you can choose between:
//! - A [`Client`], which gives you full controll and is used with a [`mio::Poll`](https://docs.rs/mio/latest/mio/struct.Poll.html).
//! - A [`SimpleClient`], which makes it simple to send one request, send many requests at the same time
//!   or stream a request.
//!
//! Note that I wrote this client myself and it is really basic
//! and doesn't handle errors very well right now.
//! The whole thing probably falls apart for websites wich use more then
//! basic HTTP 1.1 features.
//!
//! ### Supported features:
//! - Plain HTTP requests
//! - Secure HTTPS requests (using rustls)
//! - Chunked transfer encoding
//! - Nonblocking DNS lookup & HTTP requests
//! - Timeouts
//! 
//! ### Currently not implemented features:
//! - Connection keep alive
//! - maybe more...
//!
//! # Features
//!
//! The `tls` default-feature enables the use of HTTPS.
//!

mod util;
mod dns;
pub mod http;
pub mod client;
pub mod simple;

#[cfg(test)]
mod test;

pub use {
    http::*,
    client::Client,
    simple::{SimpleClient, SimpleResponse, RequestError},
};

