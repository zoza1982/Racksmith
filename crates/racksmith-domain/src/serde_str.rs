//! Serde support for types whose wire form is their `Display` / `FromStr` text.

use std::fmt::{self, Display};
use std::marker::PhantomData;
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserializer, Serializer};

/// Serializes `value` as the string produced by its `Display` implementation.
pub(crate) fn serialize<T, S>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    T: Display,
    S: Serializer,
{
    serializer.collect_str(value)
}

/// Deserializes a string and parses it with `FromStr`, reporting parse errors as serde errors.
///
/// `expecting` completes the sentence "invalid value: expected ...".
pub(crate) fn deserialize<'de, T, D>(
    deserializer: D,
    expecting: &'static str,
) -> Result<T, D::Error>
where
    T: FromStr,
    T::Err: Display,
    D: Deserializer<'de>,
{
    deserializer.deserialize_str(FromStrVisitor {
        expecting,
        target: PhantomData,
    })
}

/// Visitor that parses borrowed or owned strings without an intermediate allocation.
struct FromStrVisitor<T> {
    expecting: &'static str,
    target: PhantomData<T>,
}

impl<T> Visitor<'_> for FromStrVisitor<T>
where
    T: FromStr,
    T::Err: Display,
{
    type Value = T;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.expecting)
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<T, E> {
        text.parse().map_err(E::custom)
    }
}
