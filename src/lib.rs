
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

