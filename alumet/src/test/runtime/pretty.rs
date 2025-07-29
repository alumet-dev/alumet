use std::{any::Any, fmt::Debug};

/// A wrapper to pretty-print the panic payload returned by [`std::panic::catch_unwind`].
pub struct PrettyAny(pub Box<dyn Any + Send>);

impl Debug for PrettyAny {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Handle the common case of string messages: panic!("message here")
        // It also works with standard assertions: assert_eq!(a, b) produces a panic with a string
        if let Some(str) = self.0.downcast_ref::<&str>() {
            return f.write_str(str);
        }
        if let Some(str) = self.0.downcast_ref::<String>() {
            return f.write_str(str);
        }
        self.0.fmt(f)
    }
}
