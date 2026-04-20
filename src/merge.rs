#![allow(dead_code)]

use std::path::PathBuf;

pub use merge::Merge;

use crate::constants::UNSET_VALUE;

pub trait MergeWith<F: Fn() -> Self> {
    fn merge_with(&mut self, other: F);
}

pub trait MergeDefault {
    fn merge_default(&mut self);
}

impl<T: Merge, F: Fn() -> T> MergeWith<F> for T {
    fn merge_with(&mut self, other: F) {
        self.merge(other());
    }
}

impl<T: Merge + Default> MergeDefault for T {
    fn merge_default(&mut self) {
        self.merge(Default::default());
    }
}

pub(crate) trait Finalize {
    fn finalize(&mut self);
}

impl Finalize for Option<String> {
    fn finalize(&mut self) {
        if self.as_ref().is_some_and(|s| s.trim() == UNSET_VALUE) {
            *self = None;
        }
    }
}

impl Finalize for Option<PathBuf> {
    fn finalize(&mut self) {
        if self
            .as_ref()
            .is_some_and(|p| p.to_str().map(str::trim) == Some(UNSET_VALUE))
        {
            *self = None;
        }
    }
}

impl Finalize for Option<Vec<String>> {
    fn finalize(&mut self) {
        if let Some(vec) = self {
            if let Some(pos) = vec.iter().position(|s| s.trim() == UNSET_VALUE) {
                vec.truncate(pos);
            }
            if vec.is_empty() {
                *self = None;
            }
        }
    }
}

impl<T: Finalize> Finalize for Option<T> {
    fn finalize(&mut self) {
        if let Some(inner) = self {
            inner.finalize();
        }
    }
}
