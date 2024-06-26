pub mod naming;
pub mod threading;
pub mod versioned;
pub mod scope;

pub(crate) fn assert_send<T: Send>() { }
pub(crate) fn assert_sync<T: Sync>() { }
