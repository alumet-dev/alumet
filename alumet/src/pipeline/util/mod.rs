pub mod channel;
pub mod scope;
pub mod stream;
pub mod threading;

/// Check (at compile-time) that `T` is [`Send`].
#[allow(unused)] // used in tests
pub(crate) fn assert_send<T: Send>() {}

/// Check (at compile-time) that `T` is [`Sync`].
#[allow(unused)] // used in tests
pub(crate) fn assert_sync<T: Sync>() {}
