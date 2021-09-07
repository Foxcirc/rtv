
use rtv::Traverse;
use std::io::{Read, self};
use std::fs;

/// A test how easy it is to get the size of the files all combined
/// and them read them into a String.

#[test]
fn size() -> io::Result<()> {

    let files = Traverse::new("tests/test_env").build()?; // todo add a collect-like method to automatically open all the files instead of getting a DirEntry

    // The size of all the files together.
    let size: u64 = files.iter().map(|v| v.metadata().unwrap().len()).sum();
    let mut buff = String::with_capacity(size as usize);

    for mut file in files.iter().map(|entry| fs::File::open(entry.path()).unwrap()) {
        file.read_to_string(&mut buff).unwrap();
    }

    assert!(size as usize == buff.len());

    Ok(())

}
