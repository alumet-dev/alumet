//! Utilities for working with OS threads.

use std::{
    io,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};

use tokio::runtime::Runtime;

/// Sets the scheduling priority of the current thread to be closer to "real time".
pub fn use_realtime_scheduling() -> std::io::Result<()> {
    // On Linux, the simplest thing would be to call libc::sched_setscheduler(0, libc::SCHED_FIFO, &params).
    // However, it doesn't work on musl, nor on other UNIX-like systems such as MacOS.
    #[cfg(any(target_family = "unix"))]
    {
        let min_prio = unsafe { libc::sched_get_priority_min(libc::SCHED_FIFO) };
        let max_prio = unsafe { libc::sched_get_priority_max(libc::SCHED_FIFO) };

        // On Linux the priority will be 55, which is what we want.
        // See https://access.redhat.com/documentation/fr-fr/red_hat_enterprise_linux_for_real_time/8/html/optimizing_rhel_8_for_real_time_for_low_latency_operation/assembly_viewing-scheduling-priorities-of-running-threads_optimizing-rhel8-for-real-time-for-low-latency-operation
        let priority = (max_prio * 56 / 100).clamp(min_prio, max_prio);

        #[cfg(target_env = "musl")]
        fn sched_params(priority: i32) -> libc::sched_param {
            // With musl, the sched_param struct contains additional fields that we don't know how to initialize
            // (they are an implementation details and/or reserved fields for later use).
            // Hence, we simply initialize the whole struct to zeros.
            // SAFETY: zero is a valid value for each field of the C struct.
            let mut params: libc::sched_param = unsafe { std::mem::zeroed() };
            params.sched_priority = priority;
            params
        }

        #[cfg(not(target_env = "musl"))]
        fn sched_params(priority: i32) -> libc::sched_param {
            libc::sched_param {
                sched_priority: priority,
            }
        }

        let thread = unsafe { libc::pthread_self() };
        let params = sched_params(priority);
        let res = unsafe { libc::pthread_setschedparam(thread, libc::SCHED_FIFO, &params) };
        if res < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    #[cfg(not(target_family = "unix"))]
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "cannot increase the thread priority on this platform",
    ))
}

pub fn build_normal_runtime(worker_threads: Option<usize>) -> io::Result<Runtime> {
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder.enable_all().thread_name_fn(|| {
        static ATOMIC_ID: AtomicUsize = AtomicUsize::new(0);
        let id = ATOMIC_ID.fetch_add(1, Ordering::SeqCst);
        format!("normal-worker-{id}")
    });
    if let Some(n) = worker_threads {
        builder.worker_threads(n);
    }
    builder.build()
}

pub fn build_priority_runtime(worker_threads: Option<usize>) -> io::Result<Runtime> {
    fn resolve_application_path() -> io::Result<PathBuf> {
        std::env::current_exe()?.canonicalize()
    }

    // If `on_thread_start` fails, `builder.build()` will still return a runtime,
    // but it will be unusable. To avoid that, we store the error here and don't return Some(runtime).
    static THREAD_START_FAILURE: Mutex<Option<io::Error>> = Mutex::new(None);

    let mut builder = tokio::runtime::Builder::new_multi_thread();
    if let Some(n) = worker_threads {
        builder.worker_threads(n);
    }
    builder
            .enable_all()
            .thread_name_fn(|| {
                static ATOMIC_ID: AtomicUsize = AtomicUsize::new(0);
                let id = ATOMIC_ID.fetch_add(1, Ordering::SeqCst);
                format!("priority-worker-{id}")
            })
            .on_thread_start(|| {
                if let Err(e) = super::threading::use_realtime_scheduling() {
                    let mut failure = THREAD_START_FAILURE.lock().unwrap();
                    if failure.is_none() {
                        let hint =
                            if e.kind() == io::ErrorKind::PermissionDenied {
                                let app_path = resolve_application_path()
                                    .ok()
                                    .and_then(|p| p.to_str().map(|s| s.to_owned()))
                                    .unwrap_or(String::from("path/to/agent"));

                                indoc::formatdoc! {"
                                    This is probably caused by insufficient privileges.
                                    
                                    To fix this, you have two possibilities:
                                    1. Grant the SYS_NICE capability to the agent binary.
                                         sudo setcap cap_sys_nice+ep \"{app_path}\"
                                    
                                       Note: to grant multiple capabilities to the binary, you must put all the capabilities in the same command.
                                         sudo setcap \"cap_sys_nice+ep cap_perfmon=ep\" \"{app_path}\"
                                    
                                    2. Run the agent as root (not recommended).
                                "}
                            } else {
                                String::from("This does not seem to be caused by insufficient privileges. Please report an issue on the GitHub repository.")
                            };
                        log::error!("I tried to increase the scheduling priority of the thread in order to improve the accuracy of the measurement timing, but I failed: {e}\n{hint}");
                        log::warn!("Alumet will still work, but the time between two measurements may differ from the configuration.");
                        *failure = Some(e);
                    }
                    let current_thread = std::thread::current();
                    let thread_name = current_thread.name().unwrap_or("<unnamed>");
                    log::warn!("Unable to increase the scheduling priority of thread {thread_name}.");
                };
            });

    // Build the runtime.
    let runtime = builder.build()?;

    // Try to spawn a task to ensure that the worker threads have started properly.
    // Otherwise, builder.build() may return and the threads may fail after the failure check.
    runtime.block_on(async {
        let _ = runtime
            .spawn(tokio::time::sleep(tokio::time::Duration::from_millis(1)))
            .await;
    });

    // If the worker threads failed to start, don't use this runtime.
    if let Some(err) = THREAD_START_FAILURE.lock().unwrap().take() {
        return Err(err);
    }
    Ok(runtime)
}
