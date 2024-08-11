use crate::file::FileStat;

pub trait RollingCondition
where
    Self: std::fmt::Debug,
{
    fn should_roll(&self, stat: &FileStat) -> Result<bool, Box<dyn std::error::Error>>;
}

#[derive(Debug)]
pub struct RollingBySize {
    desired_size: usize,
}

impl RollingCondition for RollingBySize {
    fn should_roll(&self, stat: &FileStat) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(self.desired_size <= stat.len())
    }
}

impl RollingBySize {
    pub fn new(desired_size: usize) -> Self {
        if desired_size == 0 {
            panic!("`desired_size` must be greater than 0");
        }

        Self { desired_size }
    }
}

#[derive(Debug)]
pub struct RollingByDuration {
    duration: std::time::Duration,
}

impl RollingByDuration {
    pub fn new(duration: std::time::Duration) -> Self {
        if duration.is_zero() {
            panic!("`duration` must be greater than 0");
        }

        Self { duration }
    }
}

impl RollingCondition for RollingByDuration {
    fn should_roll(&self, stat: &FileStat) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(stat.created() + self.duration <= time::OffsetDateTime::now_utc())
    }
}
