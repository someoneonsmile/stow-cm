use std::path::PathBuf;

pub(crate) trait Merge<T> {
    fn merge(self, other: T) -> T;
}

pub(crate) trait MergeWith<T, F: Fn() -> T> {
    fn merge_with(self, other: F) -> T;
}

pub(crate) trait MergeDefault<T> {
    fn merge_default(self) -> T;
}

impl<T: Merge<T>, F: Fn() -> T> MergeWith<T, F> for T {
    fn merge_with(self, other: F) -> T {
        self.merge((other)())
    }
}

impl<T: Merge<T> + Default> MergeDefault<T> for T {
    fn merge_default(self) -> T {
        self.merge(Default::default())
    }
}

impl<T: Merge<T>> Merge<Self> for Option<T> {
    fn merge(self, other: Option<T>) -> Option<T> {
        // match (self, other) {
        //     (Some(a), Some(b)) => Some(a.merge(b)),
        //     (Some(a), None) => Some(a),
        //     (None, Some(b)) => Some(b),
        //     (None, None) => None,
        match (self, other) {
            (Some(a), Some(b)) => Some(a.merge(b)),
            (a, b) => a.or(b),
        }
    }
}

impl<T> Merge<Self> for Vec<T> {
    fn merge(mut self, mut other: Vec<T>) -> Vec<T> {
        self.append(&mut other);
        self
    }
}

impl Merge<Self> for PathBuf {
    fn merge(self, _other: Self) -> Self {
        self
    }
}

impl Merge<Self> for bool {
    fn merge(self, _other: bool) -> bool {
        self
    }
}
