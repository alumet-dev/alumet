use tokio::sync::mpsc::{self, error::TryRecvError};

use crate::{
    measurement::MeasurementBuffer,
    pipeline::{
        elements::{error::TransformError, transform::TransformContext},
        Transform,
    },
};

pub(super) struct WrappedTransform {
    pub transform: Box<dyn Transform>,
    pub set_rx: mpsc::Receiver<SetTransformOutputCheck>,
    pub done_tx: mpsc::Sender<TransformDone>,
}

pub struct SetTransformOutputCheck(pub Box<dyn Fn(&MeasurementBuffer) + Send>);
pub struct TransformDone;

impl Transform for WrappedTransform {
    fn apply(&mut self, measurements: &mut MeasurementBuffer, ctx: &TransformContext) -> Result<(), TransformError> {
        // run the transform
        self.transform.apply(measurements, ctx)?;

        // if set, check the output (TODO check that try_recv always see the message if send is called "just before")
        match self.set_rx.try_recv() {
            Ok(check) => {
                (check.0)(&measurements);
                self.done_tx.try_send(TransformDone).unwrap();
                Ok(())
            }
            Err(TryRecvError::Empty) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}
