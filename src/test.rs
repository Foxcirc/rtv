
use std::{future, time::Duration};

use crate::{execute, timeout};

#[test]
fn ready() {

    let future = future::ready(69);
    let value = execute(future);
    assert!(value == 69);

}

#[test]
fn pending() {

    let future = future::pending();
    let value: Option<()> = timeout(future, Duration::from_secs(1));
    assert!(value == None);

}

