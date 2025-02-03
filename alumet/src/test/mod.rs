use crate::{agent, measurement::MeasurementBuffer, metrics::Metric, pipeline::{Output, Transform}, plugin::{rust::AlumetPlugin, PluginMetadata}};

pub struct RuntimeExpectations {
    sources_to_check: Vec<(SourceName, Box<dyn Fn(&MeasurementBuffer)>)>
}

impl RuntimeExpectations {
    pub fn new(){
        todo!()
    }

    pub fn build(self) -> PluginMetadata {
        PluginMetadata {
            name: RuntimeExpectationsPlugin::name().to_owned(),
            version: RuntimeExpectationsPlugin::version().to_owned(),
            init: Box::new(move |_config| {
                // We don't care about the config BUT we need to move the data that will
                // allow the plugin to do the checks.
                Ok(Box::new(RuntimeExpectationsPlugin {
                    sources_to_check: self.sources_to_check,
                }))
            }),
            default_config: Box::new(|| Ok(None)),
        }
    }

    pub fn source_output(self, plugin: &str, source_name: &str, f: impl Fn(&MeasurementBuffer)) -> Self {
        let name = SourceName::from_parts(plugin, source_name);
        
        todo!()
    }

    pub fn source_result(self, source_name: &str, prepare: impl Fn(), check: impl Fn(&MeasurementBuffer)) -> Self{
        todo!()
    }

    pub fn transform_result(self, source_name: &str, input: impl Fn() -> (MeasurementBuffer, MeasurementOrigin), output: impl Fn(&MeasurementBuffer)) -> Self{
        todo!()
    }

}

struct RuntimeExpectationsPlugin {
    sources_to_check: Vec<(SourceName, Box<dyn Fn(&MeasurementBuffer)>)>
    transform_to_check: Vec<(TransformName, Box<dyn Fn() -> (MeasurementBuffer, MeasurementOrigin)>, Box<dyn Fn(&MeasurementBuffer)>)>
}

impl AlumetPlugin for RuntimeExpectationsPlugin {
    fn name() -> &'static str {
        todo!()
    }

    fn version() -> &'static str {
        todo!()
    }

    fn init(config: crate::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        todo!()
    }

    fn default_config() -> anyhow::Result<Option<crate::plugin::ConfigTable>> {
        todo!()
    }

    fn start(&mut self, alumet: &mut crate::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        // For each source to check, create an Alumet transform that will inspect the buffer produced by the source.
        for source in self.sources_to_check {
            alumet.add_transform(SourceCheckTransform {
                ...
            });
        }
        // For each transform to check, create an Alumet output that will inspect the buffer produced by the transform.
        for transform in self.transform_to_check {
            alumet.add_blocking_output(Box::new(TransformCheckOutput {
                ...
            }));
        }
        todo!()
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        todo!()
    }
}

struct SourceCheckTransform {
    check: Box<dyn Fn(&MeasurementBuffer)>,
    source_id: SourceId,
}
impl Transform for SourceCheckTransform {
    fn apply(&mut self, measurements: &mut MeasurementBuffer, ctx: &crate::pipeline::elements::transform::TransformContext) -> Result<(), crate::pipeline::elements::error::TransformError> {
        if matches!(ctx.origin(), Origin::Source(source_id)) {
            (self.check)(measurements);
        }
        Ok(())
    }
}

struct TransformCheckOutput {
    
}
impl Output for TransformCheckOutput {
    fn write(&mut self, measurements: &MeasurementBuffer, ctx: &crate::pipeline::elements::output::OutputContext) -> Result<(), crate::pipeline::elements::error::WriteError> {
        todo!()
    }
}


#[derive(Default)]
pub struct StartupExpectations {
    metrics: Vec<Metric>,
    plugins: Vec<String>,
    sources: Vec<(String, String, SourceType)>,
    transforms: Vec<(String, String)>,
    outputs: Vec<(String, String)>,
}

impl StartupExpectations {    
    pub(crate) fn apply(self, builder: &mut agent::Builder) {
        builder.after_plugins_start(|p| {
                for expected_metric in self.metrics {
                    let expected_name = &expected_metric.name;
                    let actual_metric = p.metrics().by_name(expected_name);
                    match actual_metric {
                        Some((_, metric_def)) => {
                            assert_eq!(metric_def.name, expected_metric.name, "MetricRegistry is inconsistent: lookup by name {} returned {:?}", expected_name, metric_def);
                            assert_eq!(metric_def.unit, expected_metric.unit, "StartupExpectations not fulfilled: metric {} should have unit {}, not {}", expected_name, expected_metric.unit, metric_def.unit);
                            assert_eq!(metric_def.value_type, expected_metric.value_type, "StartupExpectations not fulfilled: metric {} should have type {}, not {}", expected_name, expected_metric.value_type, metric_def.value_type);
                        },
                        None => {
                            panic!("StartupExpectations not fulfilled: missing metric {}", expected_name);
                        },
                    }
                }
            });
            
        builder.after_plugins_init(|plugins| {
            for plugin in self.plugins {
                assert!(plugins.iter().find(|p| p.name() == plugin ).is_some(), "StartupExpectations not fulfilled: plugin {} not found", plugin);   
            }
        });

        builder.before_operation_begin(|pipeline| {
            for source in self.sources {
                let (plugin_name, source_name, source_type) = source;
                assert_eq!(plugin_name, ...);
                assert_eq!(source_name, ...);
                assert_eq!(source_type, ...);
            }
        });
        todo!()
    }

    pub fn start_metric(mut self, metric: Metric) -> Self {
        self.metrics.push(metric);
        self
    }

    pub fn element_source(mut self, plugin_name: &str, source_name: &str, source_type: SourceType) -> Self {
        // In this plugin, check if the source is in all available sources (TODO need #79)
        self.sources.push((plugin_name.to_owned(), source_name.to_owned(), source_type));
        self
    }

    pub fn element_transform(mut self, plugin_name: &str, transform_name: &str) -> Self {
        self.transforms.push((plugin_name.to_owned(), transform_name.to_owned()));
        self
    }

    pub fn element_output(mut self, plugin_name: &str, output_name: &str) -> Self {
        self.outputs.push((plugin_name.to_owned(), output_name.to_owned()));
        self
    }

}
