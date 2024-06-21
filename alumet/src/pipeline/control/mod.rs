use naming::NameGenerator;
use output::OutputControl;
use source::SourceControl;
use tokio::{
    runtime,
    sync::mpsc::{Receiver, Sender},
};
use tokio_util::sync::CancellationToken;
use transform::TransformControl;

mod naming;
pub mod output;
pub mod source;
pub mod transform;
mod versioned;

pub struct ControlHandle {
    tx: Sender<ControlMessage>,
    shutdown: CancellationToken,
}

pub enum ControlMessage {
    Source(source::ControlMessage),
    Transform(transform::ControlMessage),
    Output(output::ControlMessage),
}

struct PipelineControl {
    rt_normal: runtime::Handle,
    rt_priority: Option<runtime::Handle>,
    name_generator: NameGenerator,
    sources: SourceControl,
    transforms: TransformControl,
    outputs: OutputControl,
}

impl PipelineControl {
    fn handle_message(&mut self, msg: ControlMessage) {
        match msg {
            ControlMessage::Source(msg) => self.sources.handle_message(msg, &self.rt_normal, &self.rt_priority),
            ControlMessage::Transform(msg) => self.transforms.handle_message(msg),
            ControlMessage::Output(msg) => self.outputs.handle_message(msg),
        }
    }

    async fn run(mut self, shutdown: CancellationToken, mut rx: Receiver<ControlMessage>) {
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => {
                    break;
                },
                _ = tokio::signal::ctrl_c() => {
                    shutdown.cancel();
                },
                message = rx.recv() => {
                    match message {
                        Some(msg) => self.handle_message(msg),
                        None => todo!("pipeline_control_loop#rx channel closed"),
                    }
                }
            }
        }
        self.shutdown();
    }
    
    fn shutdown(self) {
        self.sources.shutdown();
        self.transforms.shutdown();
        self.outputs.shutdown();
    }
}
