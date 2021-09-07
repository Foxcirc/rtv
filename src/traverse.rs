
use std::path::Path;
use std::fs::{
    self,
    File,
    DirEntry,
    OpenOptions
};
use std::io;

/// Check if this error should be ignored.
macro_rules! check {
    ($result:expr, $ignored:expr => $action:expr) => {
        match $result {
            Ok(v) => v,
            Err(e) => { if !$ignored.contains(&e.kind()) { return Err(e) } else { $action } }
        }
    };
}

/// Used to specify wich directory to traverse and with
/// what options to open the files.
/// 
/// # Examples
/// 
/// Since we only read from the file here, we don't need to specify
/// what we want to do with it.
/// 
/// ```no_run
/// 
/// use rtv::Traverse;
/// use std::io::Read;
/// 
/// Traverse::new("path/to/dir").apply(|mut file| {
///     //  It is better to use String::with_capacity with the file's size to avoid multiple allocations.
///     let mut buff = String::new();
///     file.read_to_string(&mut buff);
///     println!("{}", buff);
/// });
/// 
/// ```
#[derive(Debug, Clone)]
pub struct Traverse<A: AsRef<Path>> {
    path: A,
    options: OpenOptions,
    ignored: Vec<io::ErrorKind>
}

impl <A: AsRef<Path>>Traverse<A> {

    /// Create a new instance with the given path.
    pub fn new(path: A) -> Self {
        let mut options = OpenOptions::new();
        options.read(true);

        Self { path, options, ignored: Vec::new() }
    }

    /// A shortcut for calling [`Traverse::options`] with `OpenOptions` where
    /// the read permission is set.
    /// 
    /// `Traverse::new("path/to/dir").read(true);`
    /// 
    /// is equivalent to
    /// 
    /// `Traverse::new("path/to/dir").options(std::fs::OpenOptions::new().read(true));`
    /// 
    /// Note that it is (likely) reduntant to call this method with `true`, since
    /// reading is **enabled by default**.
    pub fn read(self, perm: bool) -> Self {
        let mut options = self.options;
        options.read(perm);
        Self { path: self.path, options, ignored: self.ignored }
    }
    
    /// A shortcut for calling [`Traverse::options`] with `OpenOptions` where
    /// the write permission is set.
    /// 
    /// `Traverse::new("path/to/dir").write(true);`
    /// 
    /// is equivalent to
    /// 
    /// `Traverse::new("path/to/dir").options(std::fs::OpenOptions::new().write(true));`
    /// 
    pub fn write(self, perm: bool) -> Self {
        let mut options = self.options;
        options.write(perm);
        Self { path: self.path, options, ignored: self.ignored }
    }

    /// Change the [`OpenOptions`] the files are opened with.
    /// 
    /// **The read option is set by default.**
    ///
    /// # Examples
    /// 
    /// This function will write "Hello world!" to every file that is contained 
    /// inside "path/to/dir" and it's subdirectories, so it needs `write`
    /// permissions.
    /// 
    /// ```no_run
    /// 
    /// use rtv::Traverse;
    /// use std::io::Write;
    /// use std::fs::OpenOptions;
    /// 
    /// Traverse::new("path/to/dir").options(OpenOptions::new().write(true)).apply(|mut file| {
    ///     write!(file, "Hello world!").unwrap();
    /// });
    /// 
    /// ```
    pub fn options(self, options: &mut OpenOptions) -> Self {
        Self { path: self.path, options: options.clone(), ignored: self.ignored }
    }
    
    /// Specifies IO errors to ignore.
    /// 
    /// After calling this function errors of the specified [`std::io::ErrorKind`] will be ignored.
    /// Ignoring an error will just skip that file / directory, depending on where the error
    /// occured.
    /// 
    /// # Examples
    /// 
    /// Here we ignore `PermissionDenied` errors, because we just want to ignore files where we
    /// don't have `read` permission. // todo make an on_error function wich takes a callback that is called if an error is ignored 
    /// 
    /// ```no_run
    /// 
    /// use rtv::Traverse;
    /// use std::io::{Read, ErrorKind};
    /// use std::fs::OpenOptions;
    /// 
    /// Traverse::new("path/to/dir").ignore(ErrorKind::PermissionDenied).apply(|mut file| {
    ///     let mut buff = String::new();
    ///     file.read_to_string(&mut buff);
    ///     println!("{}", buff);
    /// });
    /// 
    /// ```
    /// 
    pub fn ignore(self, kind: io::ErrorKind) -> Self {
        let mut ignored = self.ignored; // pusing directly onto self.ignored requires `mut self`
        ignored.push(kind);
        Self { path: self.path, options: self.options, ignored }
    }
    
    /// Call a function on every file.
    /// 
    /// This function take a callback, traverses the directory structure and 
    /// calls the callback with the opened file as argument.
    /// 
    /// For writing to files or managing other permissions, see [`Traverse::options`]. // todo add note for use with OpenOptions::create_new etc.
    /// 
    /// If an error is reported, the closure **will not get called**, since error checking // todo add the file path as an argument to the callback
    /// is done before 
    /// 
    /// # Example
    /// 
    /// ```no_run
    /// 
    /// use rtv::Traverse;
    /// use std::io::Read;
    /// 
    /// Traverse::new("path/to/dir").apply(|mut file| {
    ///     let mut buff = String::new();
    ///     file.read_to_string(&mut buff);
    ///     println!("{}", buff);
    /// });
    /// 
    /// ```
    /// 
    pub fn apply<B: FnMut(File)>(&self, mut func: B) -> io::Result<()> {
        
        let items = self.build()?;
        let mut files = Vec::with_capacity(items.len());
        
        for item in items {
            let file = check!(self.options.open(item.path()), self.ignored => continue);
            files.push(file)
        }

        // build already ignores the specified errors
        for file in files { func(file) };        

        Ok(())
    }
    
    /// Collect all files into a [`Vec`].
    /// 
    /// This function traverses the directory structure and returns a [`Vec`] containing all files.
    /// 
    /// Specifically, the returned vector contains [`DirEntry`]'s wich are **guarantied to be files**.
    /// This doesn't mean you can open them without fearing a `NotFound` error though, because the file
    /// may have beed deleted after it has been processed.
    /// 
    /// Since this function doesn't actually open files. It makes no sense combining it with
    /// [`Traverse::options`], [`Traverse::read`] or [`Traverse::write`]. // todo add runtime checks for this
    /// 
    /// # Examples
    /// 
    /// ```no_run
    /// 
    /// use rtv::Traverse;
    /// use std::io::{Read, ErrorKind};
    /// use std::fs::{OpenOptions, File};
    /// 
    /// let files = Traverse::new("path/to/dir").build().unwrap();
    /// 
    /// // iterate over the Vec and print the content of the files
    /// for item in files {
    ///     // since our items are DirEntrys we have to open them first
    ///     let mut file = File::open(item.path()).unwrap();
    ///     
    ///     let mut buff = String::new();
    ///     file.read_to_string(&mut buff);
    ///     println!("{}", buff);
    /// }
    /// 
    /// ```
    /// 
    pub fn build(&self) -> io::Result<Vec<DirEntry>> {
        
        let mut files = Vec::new();
        scan(&self.path, &mut |item| files.push(item), &self.ignored)?; // scan already ignores the specified errors
        Ok(files)
        
    }
    
}

/// Performs the recursive traversal.
fn scan<A: AsRef<Path>, C: FnMut(DirEntry)>(path: A, apply: &mut C, ignored: &Vec<io::ErrorKind>) -> io::Result<()> {
    
    let items = check!(fs::read_dir(path), ignored => return Ok(()));
    
    for item in items {
        let item = check!(item, ignored => continue); // todo use reuslt
        
        let kind = check!(item.file_type(), ignored => continue);
        
        if kind.is_file() { apply(item); }
        else if kind.is_dir() { check!(scan(item.path(), apply, ignored), ignored => continue); }
        
    }
    
    Ok(())
    
}
