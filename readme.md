
# Rtv

Rtv is a simple, minimal dependency, HTTP client that runs ontop of [mio](https://crates.io/crates/mio).
It supports fully nonblocking requests, even dns resolution is nonblocking.
You can also use it with `async` as a much more light weight (but also less robust and featurefull)
alternative to something like `hyper`.

You can either setup `mio` youself and then use a `Client` to make requests using your own `Poll`
or alternatively you can use a `SimpleClient` if you want to use `async`.
However the latter is currently supported on 64 bit unix only.

## Example (Client)

This is the main functionality that this crate provides.
You can find an example [here](https://docs.rs/rtv/latest/rtv/struct.Client.html) on docs.rs.

## Example (SimpleClient)

It is really simple to make a single request.
This is not the only functionality that this client provides though!

```rust
let mut client = SimpleClient::new()?;
let request = Request::get().host("google.com");
client.send(request).await?;
```

## Mio Httpc

This crate is similar to [mio_httpc](https://crates.io/crates/mio_httpc), however
the API is much more clean, in my opinion.
MioHttpc makes event handling, timeouts and errors very verbose, while this crate aims
to provide a much simpler interface that only requires calling one function (`pump`).
Rtv supports only a subset of `mio_httpc`'s features though and is probably not as efficient
and stable. Rtv really is a *simple* HTTP client.

## Notes

Earlier versions of this crate were completely different.
I decided to repurpose the name because I don't wanna litter my profile or crates.io.
- First `rtv` was a crate for doing **r**ecursive-file-**t**ra**v**ersal
- Then it was a crate for resolving futures (completely useless)
