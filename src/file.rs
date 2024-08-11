use std::{
    fs::{self, File as StdFile},
    io::BufWriter,
};

#[derive(Debug, Default)]
pub struct FileOpenOptions {
    truncate: bool,
}

#[derive(Debug)]
pub struct FileStat {
    created: time::OffsetDateTime,
    len: usize,
}

impl FileStat {
    #[inline]
    pub fn created(&self) -> time::OffsetDateTime {
        self.created
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }
}

#[derive(Debug)]
pub(crate) struct File {
    inner: BufWriter<StdFile>,
    stat: FileStat,
}

impl std::io::Write for File {
    #[inline]
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }

    #[inline]
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf).map(|written_bytes| {
            self.stat.len += written_bytes;
            written_bytes
        })
    }
}

impl File {
    pub(crate) fn open<P>(path: P, options: &FileOpenOptions) -> std::io::Result<Self>
    where
        P: AsRef<std::path::Path>,
    {
        fs::create_dir_all(path.as_ref().parent().unwrap())?;

        let file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .append(!options.truncate)
            .truncate(options.truncate)
            .open(path)?;

        let metadata = file.metadata()?;

        Ok(Self {
            inner: BufWriter::new(file),
            stat: FileStat {
                created: time::OffsetDateTime::from(metadata.created()?),
                len: metadata.len() as usize,
            },
        })
    }

    #[inline]
    pub(crate) fn stat<'s>(&'s self) -> &'s FileStat {
        &self.stat
    }
}
