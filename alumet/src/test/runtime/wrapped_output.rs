use std::sync::mpsc::{self, TryRecvError};

use crate::{measurement::MeasurementBuffer, pipeline::Output};

pub(super) struct WrappedOutput {
    pub output: Box<dyn Output>,
    pub set_rx: mpsc::Receiver<SetOutputOutputCheck>,
    pub done_tx: mpsc::Sender<OutputDone>,
}

pub struct SetOutputOutputCheck(pub Box<dyn Fn() + Send>);
pub struct OutputDone;

impl Output for WrappedOutput {
    fn write(
        &mut self,
        measurements: &MeasurementBuffer,
        ctx: &crate::pipeline::elements::output::OutputContext,
    ) -> Result<(), crate::pipeline::elements::error::WriteError> {
        // run the output
        self.output.write(measurements, ctx)?;

        // if set, check the output (TODO check that try_recv always see the message if send is called "just before")
        match self.set_rx.try_recv() {
            Ok(check) => {
                (check.0)();
                self.done_tx.send(OutputDone).unwrap();
                Ok(())
            }
            Err(TryRecvError::Empty) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}
