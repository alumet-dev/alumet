use std::{collections::HashSet, time::Duration};

use alumet::{
    agent::{self, plugin::PluginSet},
    pipeline::{
        control::{
            handle::SendWaitError,
            request::{self, ElementListFilter},
        },
        elements::source::trigger::TriggerSpec,
        naming::{ElementKind, ElementName, PluginName},
        Output, Source, Transform,
    },
    plugin::rust::AlumetPlugin,
    static_plugins,
};
use anyhow::anyhow;

const TIMEOUT: Duration = Duration::from_secs(1);

#[test]
fn create_source() {
    let no_plugins = PluginSet::new();
    let agent = agent::Builder::new(no_plugins).build_and_start().unwrap();
    let handle = agent.pipeline.control_handle();

    // attach a plugin name to the handle in order to be able to create elements
    let handle = handle.with_plugin(PluginName(String::from("test")));

    // create a source with the handle
    let rt = current_thread_runtime();
    let source = Box::new(DummySource);
    let trigger = TriggerSpec::at_interval(Duration::from_secs(1));
    let request = request::create_one().add_source("simple_source", source, trigger);
    rt.block_on(handle.send_wait(request, TIMEOUT))
        .expect("creation request failed");

    // check that the source has been created
    let request = request::list_elements(ElementListFilter::kind(ElementKind::Source));
    let list = rt
        .block_on(handle.send_wait(request, TIMEOUT))
        .expect("list request failed");
    assert_eq!(
        list,
        vec![ElementName::from_str(ElementKind::Source, "test", "simple_source")]
    )
}

#[test]
fn create_source_error_in_builder() {
    let no_plugins = PluginSet::new();
    let agent = agent::Builder::new(no_plugins).build_and_start().unwrap();
    let handle = agent.pipeline.control_handle();

    // attach a plugin name to the handle in order to be able to create elements
    let handle = handle.with_plugin(PluginName(String::from("test")));

    // create a source with the handle
    let rt = current_thread_runtime();
    let request =
        request::create_one().add_source_builder("simple_source", |_ctx| Err(anyhow!("error in source builder")));

    let res = rt.block_on(handle.send_wait(request, TIMEOUT));
    assert!(
        matches!(res, Err(SendWaitError::Operation(_))),
        "source creation should fail and the error should be reported by send_wait"
    );
    // TODO improve error reporting in control main_loop
    // if let Err(SendWaitError::Operation(err)) = res {
    //     assert!(err.is_element(), "error should be labelled as originating from an element");
    //     assert_eq!(
    //         err.element(),
    //         Some(&ElementName::from_str(ElementKind::Source, "test", "simple_source"))
    //     );
    // }

    // check that the source has NOT been created
    let request = request::list_elements(ElementListFilter::kind(ElementKind::Source));
    let list = rt
        .block_on(handle.send_wait(request, TIMEOUT))
        .expect("list request failed");
    assert_eq!(list, Vec::new());
}

#[test]
fn list_filter() {
    env_logger::init_from_env(env_logger::Env::default());
    let plugins = PluginSet::from(static_plugins![TestPlugin]);

    let agent = agent::Builder::new(plugins).build_and_start().unwrap();
    let handle = agent.pipeline.control_handle();

    // check that we can list elements with some filters
    let rt = current_thread_runtime();

    // First, list every element. The order is not guaranteed, use a set.
    let list_all = rt
        .block_on(handle.send_wait(request::list_elements(ElementListFilter::kind_any()), TIMEOUT))
        .unwrap();
    let list_all = HashSet::<ElementName>::from_iter(list_all);
    assert_eq!(
        list_all,
        HashSet::<ElementName>::from_iter(vec![
            ElementName::from_str(ElementKind::Source, "plugin", "dummy_src"),
            ElementName::from_str(ElementKind::Transform, "plugin", "dummy_tr"),
            ElementName::from_str(ElementKind::Output, "plugin", "dummy_out"),
        ])
    );

    // List only sources
    let list = rt
        .block_on(handle.send_wait(
            request::list_elements(ElementListFilter::kind(ElementKind::Source)),
            TIMEOUT,
        ))
        .unwrap();
    assert_eq!(
        list,
        vec![ElementName::from_str(ElementKind::Source, "plugin", "dummy_src"),]
    );

    // List only transforms
    let list = rt
        .block_on(handle.send_wait(
            request::list_elements(ElementListFilter::kind(ElementKind::Transform)),
            TIMEOUT,
        ))
        .unwrap();
    assert_eq!(
        list,
        vec![ElementName::from_str(ElementKind::Transform, "plugin", "dummy_tr"),]
    );

    // List only outputs
    let list = rt
        .block_on(handle.send_wait(
            request::list_elements(ElementListFilter::kind(ElementKind::Output)),
            TIMEOUT,
        ))
        .unwrap();
    assert_eq!(
        list,
        vec![ElementName::from_str(ElementKind::Output, "plugin", "dummy_out"),]
    );

    // Filter on name
    let list = rt
        .block_on(handle.send_wait(
            request::list_elements(ElementListFilter::kind_any().name("dummy_src")),
            TIMEOUT,
        ))
        .unwrap();
    assert_eq!(
        list,
        vec![ElementName::from_str(ElementKind::Source, "plugin", "dummy_src"),]
    );

    // Filter on name
    let list = rt
        .block_on(handle.send_wait(
            request::list_elements(ElementListFilter::kind_any().name("dummy_tr")),
            TIMEOUT,
        ))
        .unwrap();
    assert_eq!(
        list,
        vec![ElementName::from_str(ElementKind::Transform, "plugin", "dummy_tr"),]
    );

    // Filter on name
    let list = rt
        .block_on(handle.send_wait(
            request::list_elements(ElementListFilter::kind_any().name("dummy_out")),
            TIMEOUT,
        ))
        .unwrap();
    assert_eq!(
        list,
        vec![ElementName::from_str(ElementKind::Output, "plugin", "dummy_out"),]
    );

    // Filter on name pattern
    let list = rt
        .block_on(
            handle.send_wait(
                request::list_elements(
                    ElementListFilter::kind_any()
                        .name_pat(alumet::pipeline::matching::StringPattern::EndWith("src".to_owned())),
                ),
                TIMEOUT,
            ),
        )
        .unwrap();
    assert_eq!(
        list,
        vec![ElementName::from_str(ElementKind::Source, "plugin", "dummy_src"),]
    );

    // Filter on name pattern
    let list_all = rt
        .block_on(
            handle.send_wait(
                request::list_elements(
                    ElementListFilter::kind_any()
                        .name_pat(alumet::pipeline::matching::StringPattern::StartWith("dummy".to_owned())),
                ),
                TIMEOUT,
            ),
        )
        .unwrap();
    let list_all = HashSet::<ElementName>::from_iter(list_all);
    assert_eq!(
        list_all,
        HashSet::<ElementName>::from_iter(vec![
            ElementName::from_str(ElementKind::Source, "plugin", "dummy_src"),
            ElementName::from_str(ElementKind::Transform, "plugin", "dummy_tr"),
            ElementName::from_str(ElementKind::Output, "plugin", "dummy_out"),
        ])
    );
}

fn current_thread_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime should build")
}

struct DummySource;
struct DummyTransform;
struct DummyOutput;
struct TestPlugin;

impl Source for DummySource {
    fn poll(
        &mut self,
        measurements: &mut alumet::measurement::MeasurementAccumulator,
        timestamp: alumet::measurement::Timestamp,
    ) -> Result<(), alumet::pipeline::elements::error::PollError> {
        Ok(())
    }
}

impl Transform for DummyTransform {
    fn apply(
        &mut self,
        measurements: &mut alumet::measurement::MeasurementBuffer,
        ctx: &alumet::pipeline::elements::transform::TransformContext,
    ) -> Result<(), alumet::pipeline::elements::error::TransformError> {
        Ok(())
    }
}

impl Output for DummyOutput {
    fn write(
        &mut self,
        measurements: &alumet::measurement::MeasurementBuffer,
        ctx: &alumet::pipeline::elements::output::OutputContext,
    ) -> Result<(), alumet::pipeline::elements::error::WriteError> {
        Ok(())
    }
}

impl AlumetPlugin for TestPlugin {
    fn name() -> &'static str {
        "plugin"
    }

    fn version() -> &'static str {
        "0.1.0"
    }

    fn init(_config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(Self))
    }

    fn default_config() -> anyhow::Result<Option<alumet::plugin::ConfigTable>> {
        Ok(None)
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        alumet.add_source(
            "dummy_src",
            Box::new(DummySource),
            TriggerSpec::at_interval(Duration::from_secs(1)),
        )?;
        alumet.add_transform("dummy_tr", Box::new(DummyTransform))?;
        alumet.add_blocking_output("dummy_out", Box::new(DummyOutput))?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
