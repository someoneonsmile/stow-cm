use crate::config::Config;

pub trait Merge<T> {
    fn merge(self, other: &T) -> T;
}

impl<T: Merge<T> + Clone> Merge<Option<T>> for Option<T> {
    fn merge(self, other: &Option<T>) -> Option<T> {
        match (self, other) {
            (Some(a), Some(b)) => Some(a.merge(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b.clone()),
            (None, None) => None,
        }
    }
}

impl Merge<Config> for Config {
    fn merge(mut self, other: &Config) -> Config {
        self.target = self.target.or_else(|| other.target.clone());
        self.ignore = match (self.ignore, &other.ignore) {
            (Some(a), Some(b)) => Some(a.merge(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b.clone()),
            (None, None) => None,
        };
        self
    }
}

impl<T: Clone> Merge<Vec<T>> for Vec<T> {
    fn merge(mut self, other: &Vec<T>) -> Vec<T> {
        self.append(&mut other.clone());
        self
    }
}
