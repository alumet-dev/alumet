//! Agent commands.

use std::time::Duration;

use alumet::{
    agent::{Agent, AgentConfig, RunningAgent},
    plugin::rust::InvalidConfig,
};
use serde::Deserialize;

use crate::{exec_process, options::AgentModifier, relative_app_path_string};

pub fn regen_config(agent: Agent) {
    agent
        .write_default_config()
        .expect("failed to (re)generate the configuration file");
    log::info!("Configuration file (re)generated.");
}

/// Keeps the agent running until the program stops.
pub fn run(agent: RunningAgent) {
    agent.wait_for_shutdown(Duration::MAX).unwrap();
}

/// Executes a process and stops the agent when the process exits.
pub fn exec(agent: RunningAgent, program: String, args: Vec<String>) {
    // Wait for the process to exit.
    let exit_status = exec_process::exec_child(program, args).expect("the child should be waitable");
    log::info!("Child process exited with status {exit_status}, Alumet will now stop.");

    // One last measurement.
    if let Err(e) = exec_process::trigger_last_measurement(&agent.pipeline) {
        log::error!("Could not trigger one last measurement after the child's exit: {e}");
    }

    // Stop the pipeline
    agent.pipeline.control_handle().shutdown();
    agent.wait_for_shutdown(Duration::MAX).unwrap();
}

// Starts the Alumet agent.
pub fn start(agent: Agent, config: AgentConfig) -> alumet::agent::RunningAgent {
    agent.start(config).unwrap_or_else(|err| {
        log::error!("{err:?}");
        if let Some(_) = err.downcast_ref::<InvalidConfig>() {
            hint_regen_config();
        }
        panic!("ALUMET agent failed to start: {err}");
    })
}

/// Loads the agent configuration and modify the core options of Alumet accordingly.
pub fn load_config<'de, Conf: AgentModifier + Deserialize<'de>, Args: AgentModifier>(
    agent: &mut Agent,
    cli_args: Args,
) -> AgentConfig {
    // Parse the config or get the default.
    let mut config = agent
        .load_config()
        .inspect_err(|_| hint_regen_config())
        .expect("could not load the agent configuration");

    // Extract the non-plugin part of the config.
    let app_config: Conf = config
        .take_app_config()
        .try_into()
        .inspect_err(|_| hint_regen_config())
        .expect("could not parse the agent configuration");

    // Modify the agent with the config.
    app_config.apply_to(agent, &mut config);

    // Modify the agent with the CLI arguments.
    cli_args.apply_to(agent, &mut config);

    config
}

fn hint_regen_config() {
    let exe_path = relative_app_path_string();
    log::error!("HINT: You could try to regenerate the configuration by running `{} regen-config` (use --help to get more information).", exe_path.display());
}
