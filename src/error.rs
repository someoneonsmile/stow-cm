use std::error::Error;

pub type StowResult<T> = Result<T, Box<dyn Error>>;
