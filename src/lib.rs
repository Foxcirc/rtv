
//! # Rtv
//!
//! Rtv is a simple, minimal dependency HTTP client that runs ontop of [`mio`](https://docs.rs/mio/0.8.8/mio).
//!
//! This library allows you to `asyncronously` send requests using `async` or [`mio`](https://docs.rs/mio/0.8.8/mio) directly.
//! Using `mio` is verbose but allows for a much more light weight architecture and fine grained control because *you* do the event
//! handling.
//!
//! I think that using tokio is overkill for most applications. It takes ages to build and requires 100+ dependencies
//! just to get a simple http client up and running.
//!
//! Depending on how what you need you can choose between:
//! - A [`Client`], which gives you full controll and is used with a [`mio::Poll`](https://docs.rs/mio/latest/mio/struct.Poll.html).
//! - A [`SimpleClient`], which enables you to use the `async` ecosystem, still in a lightweight way.
//!
//! ### Supported features:
//! - Plain HTTP requests
//! - Secure HTTPS requests
//! - Chunked transfer encoding
//! - Nonblocking DNS lookup & HTTP requests
//! - Timeouts
//! - Lightweight, runtime independent `async` reqests
//! 
//! ### Currently **not** implemented:
//! - Connection keep alive
//! - Compression (gzip etc.)
//! - Different text encodings
//! - Url percent encoding
//! - Automatic redirects
//! - Maybe more...
//!
//! The crate currently uses google's dns server (8.8.8.8) for dns lookups.
//!
//! # Features
//!
//! The `tls` default-feature enables the use of HTTPS using rustls.
//! The `async` default-feature enables the `SimpleClient` functionality.
//!

mod util;
mod dns;
pub mod http;
pub mod client;
#[cfg(test)]
mod test;

pub use {
    http::*,
    client::*
};

#[cfg(all(unix, feature = "async"))]
pub mod simple;

#[cfg(all(unix, feature = "async"))]
pub use simple::*;
