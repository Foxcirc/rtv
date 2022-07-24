
use std::path::{Path, PathBuf};
use std::fs::{
    self,
    File,
    DirEntry,
    OpenOptions
};
use std::io;

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
/// Traverse::new("path/to/dir").apply(|mut file, _| {
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
}

impl <A: AsRef<Path>>Traverse<A> {

    /// Create a new instance with the given path.
    pub fn new(path: A) -> Self {
        let mut options = OpenOptions::new();
        options.read(true);

        Self { path, options }
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
        Self { path: self.path, options }
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
        Self { path: self.path, options }
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
    /// Traverse::new("path/to/dir").options(OpenOptions::new().write(true)).apply(|file, _| {
    ///     write!(file, "Hello world!").unwrap();
    /// });
    /// 
    /// ```
    pub fn options(self, options: &mut OpenOptions) -> Self {
        Self { path: self.path, options: options.clone() }
    }
    
    /// Call a function on every file.
    /// 
    /// This function take a callback, traverses the directory structure and 
    /// calls the callback with the opened file and the path for that file as arguments.
    /// 
    /// For writing to files or managing other permissions, see [`Traverse::options`].
    /// 
    /// If an error is encountered, the closure will get called with that error. If the closure
    /// returns an error, the traversing will stop and other directorys will not be traversed.
    /// 
    /// # Example
    /// 
    /// ```no_run
    /// 
    /// use rtv::Traverse;
    /// use std::io::Read;
    /// 
    /// Traverse::new("path/to/dir").apply(|file, _| {
    ///     let mut buff = String::new();
    ///     file?.read_to_string(&mut buff)?;
    ///     println!("{}", buff);
    /// });
    /// 
    /// ```
    /// 
    /// Here a version of that function that skips every file that cannot be opened 
    /// for some reason, instead of aborting on error.
    /// 
    /// ```no_run
    /// 
    /// use rtv::Traverse;
    /// use std::io::Read;
    /// 
    /// Traverse::new("path/to/dir").apply(|file, _| {
    ///     if let Ok(file) = file {
    ///         let mut buff = String::new();
    ///         file.read_to_string(&mut buff)?;
    ///         println!("{}", buff);
    ///     }
    /// });
    /// 
    /// ```
    /// 
    /// Although this function skips a file if it cannot be opened. It fails, if we cannot read
    /// the file!
    /// To skip on every error that is forewarded using the `?` operator, see the
    /// [`apply_skip`] and [`apply_skip_dirs`] functions.
    /// 
    pub fn apply<B: FnMut(io::Result<File>, PathBuf) -> io::Result<()>>(&self, mut func: B) -> io::Result<()> {

        scan_files(&self.path, &mut |item| {
            let path = item.path();
            let file = self.options.open(&path);
            func(file, path)
        })

    }
    
    /// Call a function on every file and skip on error.
    /// 
    /// This function take a callback, traverses the directory structure and 
    /// calls the callback with the opened directory and the path for that directory as arguments.
    /// 
    /// For writing to files or managing other permissions, see [`Traverse::options`].
    /// 
    /// If the closure returns an error, the file is skipped and traversing continues.
    /// 
    /// # Example
    /// 
    /// ```no_run
    /// 
    /// use rtv::Traverse;
    /// use std::io::Read;
    /// 
    /// Traverse::new("path/to/dir").apply_skip(|file, _| {
    ///     if let Ok(file) = file {
    ///         let mut buff = String::new();
    ///         file.read_to_string(&mut buff)?;
    ///         println!("{}", buff);
    ///     }
    /// });
    /// 
    /// ```
    /// 
    pub fn apply_skip<B: FnMut(File, PathBuf) -> io::Result<()>>(&self, mut func: B) -> io::Result<()> {
        
        scan_files_skip(&self.path, &mut |item| {
            let path = item.path();
            if let Ok(file) = self.options.open(&path) {
                func(file, path)
            } else {
                Ok(())
            }
        })

    }
    
    /// Collect all files into a [`Vec`].
    /// 
    /// This function traverses the directory structure and returns a [`Vec`] containing all files.
    /// 
    /// Specifically, the returned vector contains [`PathBuf`]'s wich are the files found.
    /// This doesn't mean you can open them without fearing a `NotFound` error though, because the file
    /// may have beed deleted after it has been processed.
    /// 
    /// Since this function doesn't actually open files. It makes no sense combining it with
    /// [`Traverse::options`], [`Traverse::read`] or [`Traverse::write`].
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
    /// for path in files {
    ///     // since our items are DirEntrys we have to open them first
    ///     let file = File::open(path).unwrap();
    ///     
    ///     let mut buff = String::new();
    ///     file.read_to_string(&mut buff);
    ///     println!("{}", buff);
    /// }
    /// 
    /// ```
    /// 
    pub fn build(&self) -> io::Result<Vec<PathBuf>> {
        
        let mut files = Vec::new();
        scan_files(&self.path, &mut |item| { files.push(item.path()); Ok(()) })?;
        Ok(files)
        
    }
    
    /// Collect all files into a [`Vec`].
    /// 
    /// This function traverses the directory structure and returns a [`Vec`] containing all the **directories**.
    ///
    /// Specifically, the returned vector contains [`PathBuf`]'s wich are the path's to the directories found.
    /// 
    pub fn build_dirs(&self) -> io::Result<Vec<PathBuf>> {
        
        let mut dirs = Vec::new();
        scan_dirs(&self.path, &mut |item| { dirs.push(item.path()); Ok(()) })?;
        Ok(dirs)
        
    }
    
}

fn scan_files<A: AsRef<Path>, C: FnMut(DirEntry) -> io::Result<()>>(path: A, apply: &mut C) -> io::Result<()> {
    scan(path, &mut |item| { if item.file_type()?.is_file() { apply(item)? } Ok(()) })
}

fn scan_dirs<A: AsRef<Path>, C: FnMut(DirEntry) -> io::Result<()>>(path: A, apply: &mut C) -> io::Result<()> {
    scan(path, &mut |item| { if item.file_type()?.is_dir() { apply(item)? } Ok(()) })
}

/// Performs the recursive traversal.
fn scan<A: AsRef<Path>, C: FnMut(DirEntry) -> io::Result<()>>(path: A, apply: &mut C) -> io::Result<()> {
    
    let items = fs::read_dir(path)?;
    
    for item in items {
        let item = item?;
        let kind = item.file_type()?;
        
        if kind.is_file() {
            apply(item)?
        } else if kind.is_dir() {
            scan(item.path(), apply)?;
            apply(item)?
        }
        
    }
    
    Ok(())
    
}

fn scan_files_skip<A: AsRef<Path>, C: FnMut(DirEntry) -> io::Result<()>>(path: A, apply: &mut C) -> io::Result<()> {
    scan_skip(path, &mut |item| { if item.file_type()?.is_file() { apply(item)? } Ok(()) })
}


/// Performs the recursive traversal.
fn scan_skip<A: AsRef<Path>, C: FnMut(DirEntry) -> io::Result<()>>(path: A, apply: &mut C) -> io::Result<()> {
    
    let items = fs::read_dir(path)?;
    
    for item in items {
        let item = item?;
        let kind = item.file_type()?;
        
        if kind.is_file() {
            apply(item).ok();
        } else if kind.is_dir() {
            scan(item.path(), apply).ok();
            apply(item).ok();
        }
        
    }
    
    Ok(())
    
}
