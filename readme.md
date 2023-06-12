
# Rtv

Rtv is a simple, minimal dependency, HTTP client that runs ontop of `mio` only.
It supports fully nonblocking requests, even dns resolution is nonblocking.

You can either setup `mio` youself and then use a `Client` to make requests using your `Poll`
or alternatively you can use a `SimpleClient` if don't need that much flexibility.

# Example

It is really simple to make a single request using a `SimpleClient`.
This is not the only functionality that `SimpleClient` provides though!

```rust
let mut client = SimpleClient::new()?;
let request = Request::get().host("google.com");
client.send(request)?;
```

# Mio Httpc

This crate is similar to [mio_httpc](https://crates.io/crates/mio_httpc), however
the API is much more clean. (In my opinion)
Rtv supports only a subset of `mio_httpc`'s features though and is probably not as efficient
and stable. Rtv really is a *simple* HTTP client.

# Notes

Earlier versions of this crate were completely different.
I decided to repurpose the name because I don't wanna litter my profile or `crates.io`.
- First `rtv` was a crate for doing **r**ecursive-file-**t**ra**v**ersal
- Then it was a crate for resolving futures (completely useless)

