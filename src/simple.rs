
use std::{io, iter, time::Duration, fmt, collections::HashMap, error};

use crate::{Client, Request, ResponseHead, ResponseState};

pub struct SimpleClient<'a> {
    io: mio::Poll,
    client: Client<'a>,
    next_id: usize,
}

impl<'a> SimpleClient<'a> {

    pub fn new() -> RequestResult<Self> {

        Ok(Self {
            io: mio::Poll::new()?,
            client: Client::new(mio::Token(0)),
            next_id: 1,
        })

    }

    pub fn stream<'d>(&'d mut self, input: impl Into<Request<'a>>) -> RequestResult<SimpleResponse<BodyReader<'a, 'd>>> {

        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        let request: Request = input.into();
        let timeout = request.timeout;

        self.client.send(&self.io, mio::Token(id), request)?;

        let mut events = mio::Events::with_capacity(2);

        let mut response_head = None;
        let mut response_data_buffer = Vec::with_capacity(128);

        'ev: loop {

            self.io.poll(&mut events, timeout)?;

            let mut read_enough = false;

            for response in self.client.pump(&self.io, &events)? {
                match response.state {
                    ResponseState::Head(head) => { read_enough = true; response_head = Some(head) },
                    ResponseState::Data(mut some_data) => response_data_buffer.append(&mut some_data),
                    ResponseState::Done => break 'ev,
                    ResponseState::Dead => return Err(RequestError::Dead),
                    ResponseState::TimedOut => return Err(RequestError::TimedOut),
                }
            }

            // we need to process all responses, since the whole request
            // might have been transmitted in one packet
            if read_enough {
                break 'ev
            }

            events.clear();

        }

        let head = response_head.expect("No header?");

        let reader = BodyReader {
            io: &mut self.io,
            client: &mut self.client,
            events: mio::Events::with_capacity(4),
            timeout,
            storage: response_data_buffer,
            is_done: false,
        };

        return Ok(SimpleResponse { head, body: reader });

    }

    pub fn send(&mut self, input: impl Into<Request<'a>>) -> RequestResult<SimpleResponse<Vec<u8>>> {

        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        let request: Request = input.into();
        let timeout = request.timeout;

        self.client.send(&self.io, mio::Token(id), request)?;

        let mut events = mio::Events::with_capacity(4);

        let mut response_head = None;
        let mut result = Vec::with_capacity(128);

        'ev: loop {

            self.io.poll(&mut events, timeout)?;

            for response in self.client.pump(&self.io, &events)? {
                match response.state {
                    ResponseState::Head(head) => response_head = Some(head),
                    ResponseState::Data(mut some_data) => result.append(&mut some_data),
                    ResponseState::Done => break 'ev,
                    ResponseState::Dead => return Err(RequestError::Dead),
                    ResponseState::TimedOut => return Err(RequestError::TimedOut),
                }
            }

            events.clear();

        }

        let head = response_head.expect("No head?");
        Ok(SimpleResponse{ head, body: result })

    }

    pub fn send_many(&mut self, input: Vec<impl Into<Request<'a>>>) -> RequestResult<Vec<RequestResult<SimpleResponse<Vec<u8>>>>> {

        let num_requests = input.len();

        let mut response_builders = vec![Some(ResponseBuilder { head: None, body: Vec::with_capacity(128) }); num_requests];
        let mut responses = Vec::with_capacity(num_requests);
        for _ in 0..num_requests { responses.push(Err(RequestError::Dead)) }

        let mut id_to_idx = HashMap::with_capacity(num_requests);

        let mut smallest_timeout: Option<Duration> = None;
        for (idx, input_item) in input.into_iter().enumerate() {

            let id = self.next_id;
            self.next_id = self.next_id.wrapping_add(1);

            let request: Request = input_item.into();

            if request.timeout.unwrap_or(Duration::MAX) < smallest_timeout.unwrap_or(Duration::MAX) {
                smallest_timeout = request.timeout;
            }

            let id = self.client.send(&self.io, mio::Token(id), request)?;
            id_to_idx.insert(id, idx);

        }

        let mut events = mio::Events::with_capacity(2 + num_requests);
        let mut counter = 0;

        'ev: loop {

            self.io.poll(&mut events, smallest_timeout)?;

            for response in self.client.pump(&self.io, &events)? {
                
                let idx = id_to_idx.get(&response.id).expect("Invalid id.");

                match response.state {
                    ResponseState::Head(head) => response_builders[*idx].as_mut().expect("No builder.").head = Some(head),
                    ResponseState::Data(mut some_data) => response_builders[*idx].as_mut().expect("No builder.").body.append(&mut some_data),
                    ResponseState::Done | ResponseState::Dead | ResponseState::TimedOut => {
                        let result = match response.state {
                            ResponseState::Done => {
                                let data = response_builders[*idx].take().expect("No builder.");
                                let head = data.head.expect("No head?!");
                                Ok(SimpleResponse { head, body: data.body })
                            },
                            ResponseState::Dead => Err(RequestError::Dead),
                            ResponseState::TimedOut => Err(RequestError::TimedOut),
                            _ => unreachable!(),
                        };
                        responses[*idx] = result;
                        counter += 1;
                        if counter == num_requests { break 'ev }
                    }
                }

            }

            events.clear();

        }

        Ok(responses)

    }

}

#[derive(Clone)]
struct ResponseBuilder {
    pub(crate) head: Option<ResponseHead>,
    pub(crate) body: Vec<u8>,
}

pub struct BodyReader<'a, 'b> {
    pub(crate) io: &'b mut mio::Poll,
    pub(crate) client: &'b mut Client<'a>,
    pub(crate) events: mio::Events,
    pub(crate) timeout: Option<Duration>,
    pub(crate) storage: Vec<u8>,
    pub(crate) is_done: bool,
}

impl<'a, 'b> io::Read for BodyReader<'a, 'b> {

    fn read(&mut self, buff: &mut [u8]) -> io::Result<usize> {

        let bytes_to_read = buff.len();

        if self.is_done {
            let storage_len = self.storage.len();
            for (src, dst) in iter::zip(self.storage.drain(..), buff.iter_mut()) { *dst = src; }
            return Ok(storage_len);
        }

        else if self.storage.len() > bytes_to_read {
            buff.copy_from_slice(&self.storage[..bytes_to_read]);
            self.storage.drain(..bytes_to_read);
            return Ok(bytes_to_read)
        }

        let bytes_from_result = bytes_to_read - self.storage.len();
        let mut result = Vec::with_capacity(128);

        'ev: loop {

            self.io.poll(&mut self.events, self.timeout)?;

            let mut read_enough = false;

            for response in self.client.pump(&self.io, &self.events)? {
                
                match response.state {
                    ResponseState::Data(mut some_data) => {
                        result.append(&mut some_data);
                        if result.len() >= bytes_from_result {
                            read_enough = true;
                        }
                    },
                    ResponseState::Done => {
                        self.is_done = true;
                        read_enough = true;
                    },
                    _ => todo!(),
                }

            }

            if read_enough {
                break 'ev
            }

            self.events.clear();

        }

        let bytes_read = result.len();

        for (src, dst) in iter::zip(self.storage.drain(..), buff.iter_mut()) { *dst = src; }

        if result.len() >= bytes_from_result {
            for (src, dst) in iter::zip(result.drain(..bytes_from_result), buff.iter_mut()) { *dst = src; }
            self.storage.append(&mut result);
            Ok(bytes_to_read)
        } else {
            for (src, dst) in iter::zip(result.drain(..result.len()), buff.iter_mut()) { *dst = src; }
            Ok(bytes_read)
        }

    }

}

#[derive(Clone)]
pub struct SimpleResponse<B> {
    pub head: ResponseHead,
    pub body: B,
}

impl<B> fmt::Debug for SimpleResponse<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SimpleResponse {{ ... }}")
    }
}

pub type RequestResult<T> = Result<T, RequestError>;

#[derive(Debug)]
pub enum RequestError {
    Io(io::Error),
    Dead,
    TimedOut,
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O Error: {}", err),
            Self::Dead => write!(f, "Dead: The server closed the connection unexpectedly."),
            Self::TimedOut => write!(f, "TimedOut: Request timed out.")
        }
    }
}

impl error::Error for RequestError {}

impl From<io::Error> for RequestError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

