
# Rtv

This crate makes it easy to "just execute" a future.
It offers two simple functions `execute` and `timeout` that
will drive a future to completion on a single thread.

You can easily view all of the source code as it is all
contained within `lib.rs`

# Example

Executing a future without a timeout is as easy as calling
`execute` with the future.

```rust
let future = std::future::ready(69);
let result = rtv::execute(future);
assert!(result == 69);
```

The `timeout` function returns an `Option` which will be `Some(T)`
when the future sucesfully completed and `None` if the time ran out.

```rust
let future = std::future::pending();
let result: Option<()> = rtv::timeout(future, std::time::Duration::from_secs(2));
assert!(result == None)
```

# Note

Earlier versions of this crate were completely different.
Rtv used to be a crate for doing **r**ecursive-file-**t**ra**v**ersal
but since it was pretty useless I decided to reuse the name for something better.

