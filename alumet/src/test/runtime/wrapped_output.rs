use std::panic::{self, AssertUnwindSafe};

use anyhow::anyhow;
use tokio::sync::mpsc::{self, error::TryRecvError};

use crate::{
    measurement::MeasurementBuffer,
    pipeline::{
        elements::{error::WriteError, output::OutputContext},
        Output,
    },
};

use super::pretty::PrettyAny;

pub(super) struct WrappedOutput {
    pub output: Box<dyn Output>,
    pub set_rx: mpsc::Receiver<SetOutputOutputCheck>,
    pub done_tx: mpsc::Sender<OutputDone>,
}

pub struct SetOutputOutputCheck(pub Box<dyn Fn() + Send>);
pub struct OutputDone;

impl Output for WrappedOutput {
    fn write(&mut self, measurements: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), WriteError> {
        let res = panic::catch_unwind(AssertUnwindSafe(|| self.test_write(measurements, ctx)));
        match res {
            Ok(Ok(ok)) => Ok(ok),
            Ok(Err(e)) => Err(e),
            Err(panic) => Err(WriteError::Fatal(anyhow!("output panicked: {:?}", PrettyAny(panic)))),
        }
    }
}

impl WrappedOutput {
    fn test_write(&mut self, measurements: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), WriteError> {
        // run the output
        self.output.write(measurements, ctx)?;

        // if set, check the output (TODO check that try_recv always see the message if send is called "just before")
        match self.set_rx.try_recv() {
            Ok(check) => {
                (check.0)();
                self.done_tx.try_send(OutputDone).unwrap();
                Ok(())
            }
            Err(TryRecvError::Empty) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}
