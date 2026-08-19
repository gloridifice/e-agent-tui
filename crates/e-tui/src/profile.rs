//! Optional frontend profiling spans.

#[cfg(not(feature = "tracy"))]
pub struct NoopSpan;

#[cfg(feature = "tracy")]
#[macro_export]
macro_rules! tracy_zone {
    ($name:literal) => {{
        match tracy_client::Client::running() {
            Some(client) => Some(client.span(tracy_client::span_location!($name), 0)),
            None => None,
        }
    }};
}

#[cfg(not(feature = "tracy"))]
#[macro_export]
macro_rules! tracy_zone {
    ($name:literal) => {{
        let _ = $name;
        None::<$crate::profile::NoopSpan>
    }};
}
