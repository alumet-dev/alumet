pub mod channel;
pub mod join_set;
pub mod naming;
pub mod scope;
pub mod threading;

/// Check (at compile-time) that `T` is [`Send`].
pub(crate) fn assert_send<T: Send>() {}

/// Check (at compile-time) that `T` is [`Sync`].
pub(crate) fn assert_sync<T: Sync>() {}
