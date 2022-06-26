use crate::config::Config;
use crate::custom_type::Flag;

pub(crate) trait Merge<T> {
    fn merge(self, other: T) -> T;
}

pub(crate) trait MergeLazy<T, F: Fn() -> T> {
    fn merge_lazy(self, other: F) -> T;
}

impl<T: Merge<T>, F: Fn() -> T> MergeLazy<T, F> for T {
    fn merge_lazy(self, other: F) -> T {
        self.merge((other)())
    }
}

impl<T: Merge<T> + Clone> Merge<Self> for Option<T> {
    fn merge(self, other: Option<T>) -> Option<T> {
        match (self, other) {
            (Some(a), Some(b)) => Some(a.merge(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    }
}

// TODO: maybe should move them to the defined file
impl Merge<Self> for Config {
    fn merge(mut self, other: Config) -> Config {
        self.target = self.target.or(other.target);
        self.ignore = self.ignore.merge(other.ignore);
        self.force = self.force.merge(other.force);
        self
    }
}

impl<T: Clone> Merge<Self> for Vec<T> {
    fn merge(mut self, mut other: Vec<T>) -> Vec<T> {
        self.append(&mut other);
        self
    }
}

impl Merge<Self> for Flag {
    fn merge(self, other: Flag) -> bool {
        self || other
    }
}
