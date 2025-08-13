use std::panic::{self, AssertUnwindSafe};

use anyhow::anyhow;
use tokio::sync::mpsc::{self, error::TryRecvError};

use crate::{
    measurement::MeasurementBuffer,
    pipeline::{
        Transform,
        elements::{error::TransformError, transform::TransformContext},
    },
};

use super::pretty::PrettyAny;

pub(super) struct WrappedTransform {
    pub transform: Box<dyn Transform>,
    pub set_rx: mpsc::Receiver<SetTransformOutputCheck>,
    pub done_tx: mpsc::Sender<TransformDone>,
}

pub struct SetTransformOutputCheck(pub Box<dyn Fn(&MeasurementBuffer) + Send>);
pub struct TransformDone;

impl Transform for WrappedTransform {
    fn apply(&mut self, measurements: &mut MeasurementBuffer, ctx: &TransformContext) -> Result<(), TransformError> {
        let res = panic::catch_unwind(AssertUnwindSafe(|| self.test_apply(measurements, ctx)));
        match res {
            Ok(Ok(ok)) => Ok(ok),
            Ok(Err(e)) => Err(e),
            Err(panic) => Err(TransformError::Fatal(anyhow!(
                "transform panicked: {:?}",
                PrettyAny(panic)
            ))),
        }
    }
}

impl WrappedTransform {
    fn test_apply(
        &mut self,
        measurements: &mut MeasurementBuffer,
        ctx: &TransformContext,
    ) -> Result<(), TransformError> {
        // run the transform
        log::trace!("applying underlying transform");
        self.transform.apply(measurements, ctx)?;

        // if set, check the output (TODO check that try_recv always see the message if send is called "just before")
        match self.set_rx.try_recv() {
            Ok(check) => {
                log::trace!("applying check");
                (check.0)(measurements);

                log::trace!("wrapped transform done");
                self.done_tx.try_send(TransformDone).unwrap();
                Ok(())
            }
            Err(TryRecvError::Empty) => {
                log::trace!("no check to perform on this operation");
                Ok(())
            }
            Err(TryRecvError::Disconnected) => {
                log::trace!("there will be no more transform checks");
                Ok(())
            }
        }
    }
}
