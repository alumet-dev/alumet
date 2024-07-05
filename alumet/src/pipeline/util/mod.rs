pub mod naming;
pub mod threading;
pub mod versioned;
pub mod scope;

/// Check (at compile-time) that `T` is [`Send`].
pub(crate) fn assert_send<T: Send>() { }

/// Check (at compile-time) that `T` is [`Sync`].
pub(crate) fn assert_sync<T: Sync>() { }
