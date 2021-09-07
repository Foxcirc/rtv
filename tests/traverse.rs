
use rtv::Traverse;
use std::io::{Read, self};
use std::fs::{OpenOptions, File};

/// This only works with the correct project structure.
#[test]
fn traverse() -> io::Result<()> {

    let trav = Traverse::new("tests/test_env").options(OpenOptions::new().write(true).read(true));
    let mut buff = String::new();
    let mut buff2 = String::new();

    trav.apply(|mut file| { file.read_to_string(&mut buff).ok(); })?;

    trav.build()?.iter().map(|v| {
        let mut file = File::open(v.path()).unwrap();
        file.read_to_string(&mut buff2).unwrap();
    }).for_each(drop);

    // the second one is for the github-actions build, wich is ubuntu
    assert!(&buff == "yes\no world!yes\nyes\nno\nyes\nhehe│\r\ncomputer\r\n│" || &buff == "yes\nyes\no world!no\nheheyes\nyes\n│\ncomputer\n│");

    assert!(buff == buff2);

    Ok(())

}
