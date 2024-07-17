// //! Utilities for working with scoped tasks.

// use crate::pipeline::{elements::output::OutputContext, Output};

// /// Spawns a blocking function with the provided arguments, and waits for it to return.
// ///
// /// ## Example
// /// ```ignore
// /// spawn_blocking_with_output(output, ctx, func).await
// /// ```
// ///
// /// The above is equivalent to:
// /// ```ignore
// /// tokio::task::spawn_blocking(move || {
// ///     func(output, ctx)
// /// }).await
// /// ```
// /// but without requiring the lifetimes of the two arguments to be `'static`.
// pub async fn spawn_blocking_with_output<F, R>(
//     output: &mut dyn Output,
//     ctx: &mut OutputContext<'_>,
//     func: F,
// ) -> Result<R, tokio::task::JoinError>
// where
//     F: FnOnce(&mut dyn super::Output, &mut super::OutputContext) -> R + Send + 'static,
//     R: Send + 'static,
// {
//     struct SendThinPointer(*mut super::OutputContext);
//     struct SendFatPointer(*mut dyn super::Output);
//     unsafe impl Send for SendThinPointer {}
//     unsafe impl Send for SendFatPointer {}
//     let out_ptr = SendFatPointer(output as *mut _ as _);
//     let ctx_ptr = SendThinPointer(ctx as *mut _ as _);
//     let rt = tokio::runtime::Handle::current();
//     rt.spawn_blocking(move || {
//         // SAFETY: we wait for the task to finish, and tokio catches the panics,
//         // therefore the pointers remain valid during the entire call to `func`.
//         let (out_ptr, ctx_ptr) = (out_ptr, ctx_ptr); // We move the wrappers, not the pointers
//         let out = unsafe { &mut *(out_ptr.0 as *mut _) };
//         let ctx = unsafe { &mut *(ctx_ptr.0 as *mut _) };
//         func(out, ctx)
//     })
//     .await
// }
