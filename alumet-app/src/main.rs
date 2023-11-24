use core::fmt;
use std::{
    error::Error,
    future::{self, Future},
    ops::{DerefMut, Deref},
    path::{PathBuf, Path},
    pin::Pin,
    sync::{mpsc, Arc, Mutex},
    time::Duration, fs::File, ffi::{c_char, CStr, CString},
};

use clap::{Parser, Subcommand};
use libloading::{Symbol, Library};
use locomen_core::{
    metric::{MeasurementBuffer, MetricRegistry},
    plugin::{Plugin, MetricSource, OutputRegistry, RegisteredSourceType, SourceRegistry, PluginInfo, ffi::ExternPluginInitFn}, config,
};
use log::{debug, error, info, log_enabled, Level};
use tokio::{
    runtime::Runtime,
    task::{futures, JoinSet}, net::unix::pipe::Receiver,
};
use tokio_stream::StreamExt;

#[derive(Parser)]
#[command(author, version, about)]
pub struct Cli {
    #[arg(long, value_name = "FILE", default_value = "alumet.toml")]
    config: PathBuf,
    
    #[arg(long, value_name = "FOLDER", default_value = "plugins")]
    plugins: PathBuf,
}

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();
    let cli = Cli::parse();
    let mut global_config = toml::from_str(cli.)

    let mut global_config = toml::Table::new();
    let test_path = std::env::args().nth(1).unwrap();
    let files = vec![Path::new(&test_path)];
    let plugins = load_plugins(files, global_config);
    // start_with_plugins(plugins);

    Ok(())
}

fn load_plugins(files: Vec<&Path>, mut global_config: toml::Table) -> Vec<PluginInfo> {
    fn load(file: &Path) -> Result<PluginInfo, Box<dyn Error>> {
        log::debug!("loading dynamic library {}", file.display());
        // load the library and the symbols we need to initialize the plugin
        // BEWARE: to load a constant of type `T` from the shared library, a `Symbol<*const T>` or `Symbol<*mut T>` must be used.
        // However, to load a function of type `fn(A,B) -> R`, a `Symbol<extern fn(A,B) -> R>` must be used.
        let lib = unsafe { Library::new(file)? };
        log::debug!("library loaded");
        let sym_name: Symbol<*const *const c_char> = unsafe { lib.get(b"PLUGIN_NAME\0")? };
        let sym_version: Symbol<*const *const c_char> = unsafe { lib.get(b"PLUGIN_VERSION\0")? };
        let sym_init: Symbol<ExternPluginInitFn> = unsafe { lib.get(b"plugin_init\0")? };

        // todo add LOCOMEN_VERSION and check that the plugin is compatible
        log::debug!("symbols loaded");

        // convert the strings to Rust strings
        log::debug!("raw PLUGIN_NAME = {:?}", unsafe{CStr::from_ptr(**sym_name)});
        log::debug!("raw PLUGIN_VERSION = {:?}", unsafe{CStr::from_ptr(**sym_version)});
        let name = unsafe { CStr::from_ptr(**sym_name) }.to_str()?.to_owned();
        let version = unsafe { CStr::from_ptr(**sym_version) }.to_str()?.to_owned();
        log::debug!("PLUGIN LOADED: {name} v{version}");
        let plugin_instance = sym_init(std::ptr::null());
        log::debug!("init called from Rust");
        Ok(todo!())
        // Ok(PluginInfo { name, version, init: *sym_init })
    }
    fn init_with_config(p: PluginInfo, plugin_config: Option<toml::Value>) -> Result<Box<dyn Plugin>, Box<dyn Error>> {
        // get the plugin config (remove to take ownership)
        // let plugin_config = global_config.remove(&p.name);
        match plugin_config {
            Some(toml::Value::Table(subconfig)) => {
                // convert the subconfig to a ffi-safe version
                let ffi_config = config::ConfigTable::new(subconfig)?;
                // initialize the plugin
                // sym_init(&ffi_config)
                todo!("sym_init")
            },
            // Some(toml::Value::Array(multiple_configs)) =>
            // todo: multiple instances of the same plugin ?
            _ => {
                Err(todo!("invalid plugin config: expected table"))
            },
        }
    }

    for f in files {
        let plugin_info = load(&f).unwrap();
        // get the plugin config (remove to take ownership)
        let plugin_config = global_config.remove(&plugin_info.name);
        // let plugin = init_with_config(plugin_info, plugin_config);
    }
    todo!("let's stop here")
}

fn start_with_plugins(plugins: Vec<Box<dyn Plugin>>) {
    let mut metrics = MetricRegistry::new();
    let mut sources = SourceRegistry::new();
    let mut outputs = OutputRegistry::new();

    // start the plugins
    start_plugins(plugins, &mut metrics, &mut sources, &mut outputs);

    log::info!("Starting metric collection...");

    // Build the multithreaded runtime
    let normal_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_time()
        .build()
        .expect("unable to start the async runtime");

    // Build the priority runtime, only on demand
    let mut priority_runtime: Option<Runtime> = None;
    fn build_priority_runtime() -> Runtime {
        increase_thread_priority().expect("the thread sched_priority must be increased for the priority runtime");
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("unable to start async runtime with realtime-priority")
    }

    // Channel to pass the measurements to different tasks
    let (tx, rx) = mpsc::channel();

    // Task that consumes the metrics measurements and sends them to the outputs
    normal_runtime.spawn(async move {
        loop {
            // get the metrics, resource info, etc.
            let mut buf = rx.recv().unwrap();
            for output in &mut outputs.outputs {
                output.write(&mut buf).unwrap();
            }
        }
    });

    // Tasks that poll the sources and send the metrics through the channels
    // We group by some characteristics (poll_interval, blocking) because they must be handled differently.
    for (key, mut sources) in sources.grouped() {
        let mut timer = tokio_timerfd::Interval::new_interval(key.poll_interval).unwrap();
        let tx = tx.clone();

        // if the task is a "priority" task, use the "priority" runtime
        let (runtime, blocking) = match key.source_type {
            RegisteredSourceType::Normal => (&normal_runtime, false),
            RegisteredSourceType::Blocking => (&normal_runtime, true),
            RegisteredSourceType::Priority => (
                &*priority_runtime.get_or_insert_with(build_priority_runtime),
                false,
            ),
        };
        if blocking {
            let guarded_sources: Vec<Arc<Mutex<Box<dyn MetricSource>>>> = sources
                .into_iter()
                .map(|src| Arc::new(Mutex::new(src)))
                .collect();
            runtime.spawn(async move {
                let mut set = JoinSet::new();
                loop {
                    // wait for the next tick
                    timer.next().await;
                    // spawn one polling tasks per source, on the "blocking" thread pool
                    for src_guard in &guarded_sources {
                        let src_guard = src_guard.clone();
                        let tx = tx.clone();
                        set.spawn_blocking(move || {
                            // lock the mutex and poll the source
                            let mut src = src_guard.lock().unwrap();
                            let mut buf = MeasurementBuffer::new();
                            src.poll(&mut buf);
                            // send the results to another task
                            tx.send(buf);
                        });
                    }
                    // wait for all the tasks to finish
                    while let Some(res) = set.join_next().await {
                        match res {
                            Ok(()) => log::debug!("blocking task finished"),
                            Err(err) => log::error!("blocking task failed {}", err),
                        }
                    }
                }
            });
        } else {
            runtime.spawn(async move {
                loop {
                    // wait for the next tick
                    timer.next().await;
                    // poll the sources
                    let mut buf = MeasurementBuffer::new();
                    for src in &mut sources {
                        src.poll(&mut buf);
                    }
                    // send the results to another task
                    tx.send(buf);
                }
            });
        }
    }
}

fn start_plugins(plugins: Vec<Box<dyn Plugin>>, metrics: &mut MetricRegistry, sources: &mut SourceRegistry, outputs: &mut OutputRegistry) {
    log::info!("Starting plugins...");
    let mut n_plugins = 0;
    for mut p in plugins {
        let name = p.name();
        let version = p.version();
        log::info!("Starting plugin {name} v{version}");
        if let Err(e) = p.start(metrics, sources, outputs) {
            log::error!("Failed to start {name} v{version}: {e}")
        } else {
            n_plugins += 1;
        }
    }

    let n_metrics = metrics.len();
    let n_sources = sources.len();
    let n_outputs = outputs.len();

    log::info!(
        "{n_plugins} plugins loaded:\n\
        \t- {n_metrics} metrics\n\
        \t- {n_sources} sources\n\
        \t- {n_outputs} outputs\n"
    );
}


/// Increases the priority of the current thread.
fn increase_thread_priority() -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let priority = 51; // from table https://access.redhat.com/documentation/fr-fr/red_hat_enterprise_linux_for_real_time/8/html/optimizing_rhel_8_for_real_time_for_low_latency_operation/assembly_viewing-scheduling-priorities-of-running-threads_optimizing-rhel8-for-real-time-for-low-latency-operation
        let params = libc::sched_param {
            sched_priority: priority,
        };
        let res = unsafe { libc::sched_setscheduler(0, libc::SCHED_FIFO, &params) };
        if res < 0 {
            return Err(std::io::Error::last_os_error());
        } else {
            Ok(())
        }
    }
    #[cfg(not(target_os = "linux"))]
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "cannot increase the thread priority on this platform"))
}
