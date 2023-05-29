
//! # Rtv
//!
//! Rtv is a crate to "just execute" a future, on a single thread,
//! without any additional dependencies.
//! This crate tries to avoid unsafe as much as possible.
//! You can easily view all of the source code as it is all contained within `lib.rs`
//!
//! There are two available functions:
//! - [`execute`] to simplty execute a future
//! - [`timeout`] to execute a future with a given timoeut
//!

use std::{future::Future, task::{Context, Waker, Poll}, sync::mpsc::{sync_channel, RecvTimeoutError}, time::{Duration, Instant}};

#[cfg(test)]
mod test;

/// execute a future
///
/// # Example
///
/// ```rust
/// let future = std::future::ready(69);
/// let result = rtv::execute(future);
/// assert!(result == 69);
/// ```
#[inline]
pub fn execute<T>(future: impl Future<Output = T> + Unpin) -> T {
    run(future, Duration::MAX).expect("Duration::MAX elapsed")
}

/// execute a future with given timeout
///
/// # Example
///
/// This code will finish after `2` seconds.
///
/// ```rust
/// let future = std::future::pending();
/// let result: Option<()> = rtv::timeout(future, std::time::Duration::from_secs(2));
/// assert!(result == None)
/// ```
#[inline]
pub fn timeout<T>(future: impl Future<Output = T> + Unpin, timeout: Duration) -> Option<T> {
    run(future, timeout)
}

fn run<T>(mut future: impl Future<Output = T> + Unpin, mut timeout: Duration) -> Option<T> {

    let mut pinned = Box::pin(&mut future);
    let (sender, waiter) = sync_channel(0);

    let raw_waker = waker::new(sender);
    let waker = unsafe { Waker::from_raw(raw_waker) };
    let mut context = Context::from_waker(&waker);

    let start = Instant::now();

    loop {

        let value = pinned.as_mut().poll(&mut context);
        match value {
            Poll::Pending => match waiter.recv_timeout(timeout) {
                Ok(()) => timeout -= Instant::now() - start,
                Err(RecvTimeoutError::Timeout) => return None,
                Err(RecvTimeoutError::Disconnected) => panic!("Channel disconnected")
            },
            Poll::Ready(result) => return Some(result),
        }

    }

}

mod waker {

    use std::{task::{RawWaker, RawWakerVTable}, sync::mpsc::SyncSender, mem::ManuallyDrop};

    const VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, waker_drop);
    type WakerSender = ManuallyDrop<SyncSender<()>>;

    pub(crate) fn new(sender: SyncSender<()>) -> RawWaker {
        let value = ManuallyDrop::new(sender);
        RawWaker::new(&value as *const WakerSender as *const (), &VTABLE)
    }

    unsafe fn clone(data: *const ()) -> RawWaker {
        let value = sender(data).clone();
        RawWaker::new(&value as *const WakerSender as *const (), &VTABLE)
    }

    unsafe fn wake(data: *const ()) {
        wake_by_ref(data);
        waker_drop(data);
    }

    unsafe fn wake_by_ref(data: *const ()) {
        sender(data).send(()).expect("Channel disconnected")
    }

    unsafe fn waker_drop(data: *const ()) {
        ManuallyDrop::drop(sender(data))
    }

    #[track_caller]
    unsafe fn sender<'d>(data: *const ()) -> &'d mut WakerSender {
        (data as *mut WakerSender).as_mut().expect("Cannot dereference `sender` pointer")
    }

}

