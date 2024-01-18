
//! This module contains a [`SimpleClient`] that allows sending simple requests.

use std::{fmt, io::{self, Read, Write}, string, thread, sync::{Arc, Mutex}, collections::{HashMap, VecDeque}, task::{self, Waker, Poll}, future, pin::Pin};
use futures_lite::AsyncReadExt;

use crate::{Client, ResponseHead, ResponseState, RawRequest, util::wouldblock};

/// A simpler HTTP client that handles I/O events for you.
///
/// The `SimpleClient` allows you to:
///     1. Send a [`single`](SimpleClient::send) request and block until the response has arrived.
///     2. Send [`many`](SimpleClient::many) requests at the same time and block until all responses have arrived.
///     3. Send a single request and [`stream`](SimpleClient::stream) the request body.
///
/// # Example
///
/// It is really easy to send a single request.
///
/// ```rust
/// let mut client = SimpleClient::new()?;
/// let resp = client.send(Request::get("example.com"))?;
/// let body_str = String::from_utf8(resp.body); // note: not all websites use UTF-8!
/// println!("{}", body_str);
/// ```

// todo: the whole simple.rs is available on 64bit unix only due to mio::unix::Pipe being used
pub struct SimpleClient {
    reaper: Option<thread::JoinHandle<()>>,
    sender: mio::unix::pipe::Sender,
}

impl Drop for SimpleClient {
    fn drop(&mut self) {
        self.shutdown();
        self.reaper.take().unwrap().join().unwrap();
    }
}

struct SimpleRequestState {
    pub request: Option<RawRequest>,
    pub resps: VecDeque<ResponseState>,
    pub waker: Option<Waker>,
}

impl SimpleClient {

    const CLIENT:   mio::Token = mio::Token(0);
    const RECEIVER: mio::Token = mio::Token(1);
    const STARTID: usize = 2;

    /// Creates a new client
    ///
    /// An error is a fatal failure and probably means that the system doesn't support all necessary functionality.
    pub fn new() -> io::Result<Self> {

        let mut io = mio::Poll::new()?;
        let (sender, mut receiver) = mio::unix::pipe::new()?;

        // io.registry().register(&mut sender, Self::SENDER, mio::Interest::WRITABLE);
        // ^^^ we don't register the sender, since we will write to it in blocking mode 
        sender.set_nonblocking(false).unwrap();

        io.registry().register(&mut receiver, Self::RECEIVER, mio::Interest::READABLE)?;

        Ok(Self {
            reaper: Some(thread::spawn(move || {

                let mut client = Client::new(Self::CLIENT);
                let mut next_id = Self::STARTID;

                let mut requests = HashMap::with_capacity(8);

                loop {

                    let mut events = mio::Events::with_capacity(32);
                    io.poll(&mut events, None).unwrap();

                    'events: for event in events.iter() {

                        if event.token() == Self::RECEIVER {

                            loop {

                                let mut buff = [0; 8];

                                match receiver.read(&mut buff) {
                                    Ok(_bytes_read) => assert!(_bytes_read == 8),
                                    Err(ref err) if wouldblock(err) => break 'events,
                                    Err(err) => panic!("{}", err),
                                };

                                // writing all zeroes signals that we should shutdown
                                // we shut down without waiting for any further events
                                if buff == [0; 8] {
                                    return
                                };

                                let request_state = unsafe { Arc::from_raw(
                                    u64::from_ne_bytes(buff) as *mut Mutex<SimpleRequestState>
                                ) };
                                let mut guard = request_state.lock().unwrap();

                                let token = next_id;
                                next_id += 1;

                                let request = guard.request.take().unwrap();
                                let id = client.send(&io, mio::Token(token), request).unwrap(); // todo: can someting be done about all these unwraps

                                drop(guard);

                                requests.insert(id, request_state);

                            }

                        }                        

                    }

                    for resp in client.pump(&io, &events).unwrap() {

                        let is_finished = resp.state.is_finished();

                        let request_state = requests.get(&resp.id).unwrap();
                        let mut guard = request_state.lock().unwrap();
                        guard.resps.push_back(resp.state);
                        if let Some(ref waker) = guard.waker {
                            waker.wake_by_ref();
                        }
                        drop(guard);

                        if is_finished {
                            requests.remove(&resp.id);
                        }

                    };
                    
                }
                
            })),
            sender
        })

    }

    /// Send a single request.
    ///
    /// This method will send a single request and block until the
    /// response arrives.
    pub async fn send(&mut self, input: impl Into<RawRequest>) -> io::Result<SimpleResponse<Vec<u8>>> {

        let mut response = self.stream(input).await?;
        let mut buff = Vec::with_capacity(2048);
        response.body.read_to_end(&mut buff).await?;
        Ok(SimpleResponse {
            head: response.head,
            body: buff,
        })

    }

    // todo: make a function that returns a Future

    /// Stream a single request.
    ///
    /// This method will send a single request and return a response once the
    /// [`ResponseHead`] has been transmitted.
    /// The response will contain a [`BodyReader`] as the `body` which implements
    /// the [`AsyncRead`] trait.
    ///
    /// You can receive large responses packet-by-packet using this method.
    pub async fn stream<'d>(&'d mut self, input: impl Into<RawRequest>) -> io::Result<SimpleResponse<BodyReader>> {

        let request = input.into();

        let request_state = Arc::new(Mutex::new(SimpleRequestState {
            request: Some(request),
            resps: VecDeque::new(),
            waker: None
        }));

        let reaper_clone = Arc::clone(&request_state);
        self.sender.write_all(&(Arc::into_raw(reaper_clone) as u64).to_ne_bytes()).unwrap();

        let head = future::poll_fn(|ctx| {

            let mut guard = request_state.lock().unwrap();

            guard.waker = Some(ctx.waker().clone());

            if let Some(resp) = guard.resps.pop_front() {
                let result = match resp {
                    ResponseState::Head(head) => Ok(head),
                    error_or_data => Err(error_or_data.into_io_error().unwrap())
                };
                Poll::Ready(result)
            } else {
                Poll::Pending
            }

        }).await?;
        
        let reader = BodyReader {
            request_state,
        };

        return Ok(SimpleResponse { head, body: reader });

    }

    fn shutdown(&mut self) {
        // indicates to the reaper thread that it should shut itself down
        self.sender.write_all(&[0; 8]).unwrap();
    }

}

/// Allows streaming the body of a request.
///
/// This does some internal buffering.
/// For more information see [`SimpleClient::stream`].
pub struct BodyReader {
    request_state: Arc<Mutex<SimpleRequestState>>,
}

impl futures_io::AsyncRead for BodyReader {

    fn poll_read(self: Pin<&mut Self>, ctx: &mut task::Context<'_>, buff: &mut [u8]) -> Poll<io::Result<usize>> {

        let mut guard = self.request_state.lock().unwrap();

        if let Some(ref mut waker) = guard.waker {
            waker.clone_from(ctx.waker()); // this clone from is optimized, see Waker::will_wake
        } else {
            unreachable!()
        }

        // read some data, only removing the response entry if one chunk of
        // data was fully read
        if let Some(resp) = guard.resps.front_mut() {
            let result = match resp {
                ResponseState::Head(..) => unreachable!(),
                ResponseState::Data(data) => {
                    let to_copy = data.len().min(buff.len());
                    buff[..to_copy].copy_from_slice(&data[..to_copy]);
                    data.truncate(data.len() - to_copy);
                    if data.len() == 0 {
                        guard.resps.pop_front();
                    }
                    Ok(to_copy)
                },
                ResponseState::Done => Ok(0),
                err => Err(err.into_io_error().unwrap())
            };
            drop(guard);
            Poll::Ready(result)
        } else {
            drop(guard);
            Poll::Pending
        }
        
    }

}

/// A simple response.
///
/// Use the alternate debug formatter `{:#?}` to print out verbose information
/// including all headers and more.
///
/// The `body` may be a [`Vec<u8>`](std::vec::Vec), or a [`BodyReader`].
#[derive(Clone)]
pub struct SimpleResponse<B> {
    pub head: ResponseHead,
    pub body: B,
}

impl SimpleResponse<Vec<u8>> {

    /// Convert the request body into a `String`.
    /// Note that the data is assumed to be valid utf8. Text encodings
    /// are not handeled by this crate.
    pub fn into_string(self) -> Result<String, string::FromUtf8Error> {
        String::from_utf8(self.body)
    }

}

impl<B> fmt::Debug for SimpleResponse<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            write!(f, "SimpleResponse {{ head: {:?}, ... }}", self.head)
        } else {
            write!(f, "SimpleResponse {{ status: {}, {}, ... }}", self.head.status.code, self.head.status.reason)
        }
    }
}

