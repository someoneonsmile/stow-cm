use crate::config::Config;
use crate::custom_type::Flag;

pub trait Merge<T> {
    fn merge(self, other: T) -> T;
}

impl<T: Merge<T> + Clone> Merge<Option<T>> for Option<T> {
    fn merge(self, other: Option<T>) -> Option<T> {
        match (self, other) {
            (Some(a), Some(b)) => Some(a.merge(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b.clone()),
            (None, None) => None,
        }
    }
}

// TODO: maybe should move them to the defined file
impl Merge<Config> for Config {
    fn merge(mut self, other: Config) -> Config {
        self.target = self.target.or_else(|| other.target.clone());
        self.ignore = self.ignore.merge(other.ignore);
        self.force = self.force.merge(other.force);
        self
    }
}

impl<T: Clone> Merge<Vec<T>> for Vec<T> {
    fn merge(mut self, mut other: Vec<T>) -> Vec<T> {
        self.append(&mut other);
        self
    }
}

impl Merge<Flag> for Flag {
    fn merge(self, other: Flag) -> bool {
        self || other
    }
}
