use std::{fs, io::Write, path::Path};

use condition::RollingCondition;

use crate::file::{File, FileOpenOptions};

pub mod condition;

pub trait RollingFileNameProvider: std::fmt::Debug {
    fn acceptable(&self, file_name: &str)
        -> Result<bool, Box<dyn std::error::Error + Send + Sync>>;

    fn next_file_name(&mut self) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;
}

#[derive(Debug)]
pub struct DefaultRollingFileNameProvider {
    pub(self) file_name_prefix: Option<String>,
    pub(self) file_name_suffix: Option<String>,
    pub(self) file_name_datetime_format: String,
    pub(self) prev_file_name_datetime: Option<String>,
    pub(self) prev_file_name_datetime_hits: u64,
}

impl RollingFileNameProvider for DefaultRollingFileNameProvider {
    fn acceptable(
        &self,
        file_name: &str,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(ref file_name_prefix) = self.file_name_prefix {
            if !file_name.starts_with(file_name_prefix) {
                return Ok(false);
            }
        }

        if let Some(ref file_name_suffix) = self.file_name_suffix {
            if !file_name.ends_with(file_name_suffix) {
                return Ok(false);
            }
        }

        Ok(true)
    }

    fn next_file_name(&mut self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let prefix = self
            .file_name_prefix
            .as_ref()
            .map(AsRef::as_ref)
            .unwrap_or("");

        let suffix = self
            .file_name_suffix
            .as_ref()
            .map(AsRef::as_ref)
            .unwrap_or("");

        let datetime = time::OffsetDateTime::now_utc().format(&time::format_description::parse(
            &self.file_name_datetime_format,
        )?)?;

        if self.prev_file_name_datetime.as_ref() == Some(&datetime) {
            self.prev_file_name_datetime_hits += 1;
        } else {
            self.prev_file_name_datetime = Some(datetime);
            self.prev_file_name_datetime_hits = 0;
        }

        Ok(if self.prev_file_name_datetime_hits == 0 {
            format!(
                "{prefix}{}{suffix}",
                self.prev_file_name_datetime.as_ref().unwrap()
            )
        } else {
            format!(
                "{prefix}{}-{}{suffix}",
                self.prev_file_name_datetime.as_ref().unwrap(),
                self.prev_file_name_datetime_hits
            )
        })
    }
}

#[derive(Debug, Default)]
pub struct DefaultRollingFileNameProviderBuilder {
    file_name_suffix: Option<String>,
    file_name_prefix: Option<String>,
    file_name_datetime_format: Option<String>,
}

impl DefaultRollingFileNameProviderBuilder {
    pub fn file_name_prefix(mut self, prefix: impl ToString) -> Self {
        self.file_name_prefix = Some(prefix.to_string());
        self
    }

    pub fn file_name_suffix(mut self, suffix: impl ToString) -> Self {
        self.file_name_suffix = Some(suffix.to_string());
        self
    }
    pub fn file_name_datetime_format(mut self, format: impl ToString) -> Self {
        self.file_name_datetime_format = Some(format.to_string());
        self
    }

    pub fn finish(self) -> DefaultRollingFileNameProvider {
        DefaultRollingFileNameProvider {
            file_name_prefix: self.file_name_prefix,
            file_name_suffix: self.file_name_suffix,
            file_name_datetime_format: self
                .file_name_datetime_format
                .unwrap_or_else(|| "[year][month][day]T[hour][minute][second]".into()),
            prev_file_name_datetime: None,
            prev_file_name_datetime_hits: 0,
        }
    }
}

#[derive(Debug)]
pub struct RollingFile<T: RollingFileNameProvider> {
    pub(self) file: Option<File>,
    pub(self) directory: String,
    pub(self) file_name_provider: T,
    pub(self) file_open_options: FileOpenOptions,
    pub(self) max_file_count: Option<usize>,
    pub(self) rolling_conditions: Vec<Box<dyn RollingCondition + Send + Sync>>,
}

impl<T: RollingFileNameProvider> RollingFile<T> {
    pub fn should_roll(&self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        self.file.as_ref().map_or(Ok(true), |file| {
            for condition in &self.rolling_conditions {
                if condition.should_roll(file.stat())? {
                    return Ok(true);
                }
            }
            Ok(false)
        })
    }

    pub fn roll(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.open_next_file()?;
        self.something()?;
        Ok(())
    }

    fn open_next_file(&mut self) -> std::io::Result<()> {
        self.flush()?;
        self.file = None;

        let file_name = self
            .file_name_provider
            .next_file_name()
            .map_err(std::io::Error::other)?;

        self.file = Some(File::open(
            Path::new(&self.directory).join(&file_name),
            &self.file_open_options,
        )?);
        Ok(())
    }

    fn something(&self) -> std::io::Result<()> {
        if self.max_file_count.is_none() {
            return Ok(());
        }

        let entries: Result<Vec<_>, _> = fs::read_dir(&self.directory)?
            .map(|entry| -> std::io::Result<_> {
                let entry = entry?;
                let metadata = fs::metadata(entry.path())?;
                Ok((entry, metadata))
            })
            .collect();

        let mut log_files = entries?
            .into_iter()
            .filter_map(|(entry, metadata)| {
                if !metadata.is_file() {
                    return None;
                }

                match self
                    .file_name_provider
                    .acceptable(entry.file_name().to_str().unwrap())
                {
                    Ok(true) => (),
                    Ok(false) => return None,
                    Err(e) => return Some(Err(std::io::Error::other(e))),
                }

                Some(match metadata.created() {
                    Ok(created) => Ok((entry, created)),
                    Err(e) => Err(e),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        log_files.sort_unstable_by_key(|(_, created)| std::cmp::Reverse(created.clone()));
        for (log_file, _) in log_files.into_iter().skip(self.max_file_count.unwrap()) {
            fs::remove_file(log_file.path())?;
        }

        Ok(())
    }

    fn writer<'s>(
        &mut self,
    ) -> Result<&mut impl std::io::Write, Box<dyn std::error::Error + Send + Sync>> {
        while self.should_roll()? {
            self.roll()?;
        }
        Ok(self.file.as_mut().unwrap())
    }
}

impl<T: RollingFileNameProvider> std::io::Write for RollingFile<T> {
    #[inline]
    fn flush(&mut self) -> std::io::Result<()> {
        match self.file.as_mut() {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }

    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.writer().map_err(std::io::Error::other)?.write(buf)
    }
}

#[derive(Debug)]
pub struct RollingFileBuilder<T: RollingFileNameProvider> {
    directory: Option<String>,
    file_name_provider: Option<T>,
    file_open_options: Option<FileOpenOptions>,
    max_file_count: Option<usize>,
    rolling_conditions: Vec<Box<dyn RollingCondition + Send + Sync>>,
}

impl<T: RollingFileNameProvider> RollingFileBuilder<T> {
    pub fn new() -> Self {
        Self {
            directory: None,
            file_name_provider: None,
            file_open_options: None,
            max_file_count: None,
            rolling_conditions: Default::default(),
        }
    }

    pub fn directory(mut self, directory: impl ToString) -> Self {
        self.directory = Some(directory.to_string());
        self
    }

    pub fn file_name_provider(mut self, file_name_provider: T) -> Self {
        self.file_name_provider = Some(file_name_provider);
        self
    }

    pub fn file_open_options(mut self, options: FileOpenOptions) -> Self {
        self.file_open_options = Some(options);
        self
    }

    pub fn max_file_count(mut self, max_file_count: usize) -> Self {
        self.max_file_count = Some(max_file_count);
        self
    }

    pub fn rolling_condition(
        mut self,
        cond: impl RollingCondition + Send + Sync + 'static,
    ) -> Self {
        self.rolling_conditions.push(Box::new(cond));
        self
    }

    pub fn finish(self) -> Result<RollingFile<T>, Box<dyn std::error::Error + Send + Sync>> {
        if self.file_name_provider.is_none() {
            return Err("`file_name_provider` is required".into());
        }

        Ok(RollingFile::<T> {
            file: None,
            directory: self.directory.unwrap_or_else(|| ".".into()),
            file_name_provider: self.file_name_provider.unwrap(),
            file_open_options: self.file_open_options.unwrap_or_else(Default::default),
            max_file_count: self.max_file_count,
            rolling_conditions: self.rolling_conditions,
        })
    }
}
