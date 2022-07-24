
# Rtv

![build](https://github.com/foxcirc/rtv/actions/workflows/rust.yml/badge.svg) ![crates](https://img.shields.io/static/v1?label=crates.io&message=0.2.1&color=blue)

This is a rust crate wich makes it easy to recursively traverse a directory
or in other words, to iterate over a directory tree.

That means looking at every file inside a directory and it's subdirectories.
For example consider this layout:

```
    test_env
    │   file1
    │   file2
    │   file3
    │
    ├───folder1
    │   │   file4
    │   │   file5
    │   │
    │   └───folder3
    │           file7
    │
    └───folder2
            file6
```

This crate provides functions to iterate over all the files, from `file1` to `file7`.

These methods are exposed through the `Traverse` struct.

Here a small function that goes trough every file inside `path/to/dir` and its subdirectories and
prints the content.

```rust

use rtv::Traverse;
use std::io::Read;

Traverse::new("path/to/dir").apply(|mut file, _| {
    let mut buff = String::new();
    file.read_to_string(&mut buff);
    println!("{}", buff);
});

```

# Changelog

## 0.2.0 -> 0.2.1
- Added the `scan_dirs` function

## 0.1.2 -> 0.2.0
- The callback the `apply` function takes, now gets the path to the file.
- The `build` function now returns a `Vec<PathBuf>` instead of `Vec<DirEntry>`.

## 0.0.0 -> 0.1.2
- Basic functionality.
