pub mod channel;
pub mod matching;
pub mod naming;
pub mod scope;
pub mod stream;
pub mod threading;

/// Check (at compile-time) that `T` is [`Send`].
#[allow(unused)] // uesd in tests
pub(crate) fn assert_send<T: Send>() {}

/// Check (at compile-time) that `T` is [`Sync`].
#[allow(unused)] // used in tests
pub(crate) fn assert_sync<T: Sync>() {}
