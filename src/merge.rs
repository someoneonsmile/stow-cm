#![allow(dead_code)]

pub use merge::Merge;

pub trait MergeWith<F: Fn() -> Self> {
    fn merge_with(&mut self, other: F);
}

pub trait MergeDefault {
    fn merge_default(&mut self);
}

impl<T: Merge, F: Fn() -> T> MergeWith<F> for T {
    fn merge_with(&mut self, other: F) {
        self.merge((other)());
    }
}

impl<T: Merge + Default> MergeDefault for T {
    fn merge_default(&mut self) {
        self.merge(Default::default());
    }
}

pub mod strategy {
    pub fn option_deep<T>(f: fn(&mut T, T)) -> impl Fn(&mut Option<T>, Option<T>) {
        move |left: &mut Option<T>, right: Option<T>| {
            if let Some(new) = right {
                if let Some(original) = left {
                    f(original, new);
                } else {
                    *left = Some(new);
                }
            }
        }
    }
}
