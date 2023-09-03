use std::fmt::Display;

use anyhow::Context;

use crate::error::Result;

macro_rules! here_str {
    () => {
        concat!("at ", file!(), " line ", line!(), " column ", column!())
    };
}

// from https://github.com/dtolnay/anyhow/issues/22
macro_rules! here {
    () => {
        &Location {
            file: file!(),
            line: line!(),
            column: column!(),
        }
    };
}

pub(crate) struct Location {
    file: &'static str,
    line: u32,
    column: u32,
}

impl Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "at {} line {} column {}",
            self.file, self.line, self.column
        ))
    }
}

pub(crate) trait ErrorLocation<T> {
    fn location(self, loc: &Location) -> Result<T>;
}

impl<T, E> ErrorLocation<T> for Result<T, E>
where
    E: Display,
    Result<T, E>: Context<T, E>,
{
    fn location(self, loc: &Location) -> Result<T> {
        let msg = self.as_ref().err().map(ToString::to_string);
        self.with_context(|| format!("{} {}", msg.unwrap(), loc))
    }
}
