
/*! 

This crate enables you to *easily* traverse a directory.

Use the [`Traverse`] struct to get all the files.
You can either build them into a [`Vec`] or specify a callback function
to call on every file.

You can also choose to ignore some errors or open files with custom permissions.

This function goes trough every file inside `path/to/dir` and its subdirectories and
prints the content.

```rust

use rtv::Traverse;
use std::io::Read;

//  It is better to use String::with_capacity with the file's size to avoid multiple allocations.
let mut buff = String::new();

Traverse::new("path/to/dir").apply(|mut file| {
    file.read_to_string(&mut buff);
    println!("{}", buff);
});

```
   
*/

mod traverse;

pub use traverse::Traverse;

