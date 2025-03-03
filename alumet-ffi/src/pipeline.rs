use libc::c_void;

use super::{DropFn, FfiOutputContext, FfiTransformContext, OutputWriteFn, SourcePollFn, TransformApplyFn};
use alumet::{
    measurement::{MeasurementAccumulator, MeasurementBuffer},
    pipeline::{
        self,
        elements::{
            error,
            output::{self, error::WriteError},
            transform::{self, TransformError},
        },
    },
};

pub(crate) struct FfiSource {
    pub data: *mut c_void,
    pub poll_fn: SourcePollFn,
    pub drop_fn: Option<DropFn>,
}
pub(crate) struct FfiTransform {
    pub data: *mut c_void,
    pub apply_fn: TransformApplyFn,
    pub drop_fn: Option<DropFn>,
}
pub(crate) struct FfiOutput {
    pub data: *mut c_void,
    pub write_fn: OutputWriteFn,
    pub drop_fn: Option<DropFn>,
}
// To be safely `Send`, sources/transforms/outputs may not use any thread-local storage,
// and the `data` pointer must not be shared with other threads.
// When implementing a non-Rust plugin, this has to be checked manually.
unsafe impl Send for FfiSource {}
unsafe impl Send for FfiTransform {}
unsafe impl Send for FfiOutput {}

impl pipeline::Source for FfiSource {
    fn poll(
        &mut self,
        into: &mut MeasurementAccumulator,
        time: alumet::measurement::Timestamp,
    ) -> Result<(), error::PollError> {
        (self.poll_fn)(self.data, into, time.into());
        Ok(())
    }
}
impl pipeline::Transform for FfiTransform {
    fn apply(
        &mut self,
        measurements: &mut MeasurementBuffer,
        ctx: &transform::TransformContext,
    ) -> Result<(), TransformError> {
        let ffi_ctx = FfiTransformContext { inner: ctx };
        (self.apply_fn)(self.data, measurements, &ffi_ctx);
        Ok(())
    }
}
impl pipeline::Output for FfiOutput {
    fn write(&mut self, measurements: &MeasurementBuffer, ctx: &output::OutputContext) -> Result<(), WriteError> {
        let ffi_ctx = FfiOutputContext { inner: ctx };
        (self.write_fn)(self.data, measurements, &ffi_ctx);
        Ok(())
    }
}

impl Drop for FfiSource {
    fn drop(&mut self) {
        if let Some(drop) = self.drop_fn {
            unsafe { drop(self.data) };
        }
    }
}
impl Drop for FfiTransform {
    fn drop(&mut self) {
        if let Some(drop) = self.drop_fn {
            unsafe { drop(self.data) };
        }
    }
}
impl Drop for FfiOutput {
    fn drop(&mut self) {
        if let Some(drop) = self.drop_fn {
            unsafe { drop(self.data) };
        }
    }
}
