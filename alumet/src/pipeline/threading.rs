//! Utilities for working with OS threads.

/// Increases the priority of the current thread.
pub fn increase_thread_priority() -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let priority = 55; // from table https://access.redhat.com/documentation/fr-fr/red_hat_enterprise_linux_for_real_time/8/html/optimizing_rhel_8_for_real_time_for_low_latency_operation/assembly_viewing-scheduling-priorities-of-running-threads_optimizing-rhel8-for-real-time-for-low-latency-operation
        let params = libc::sched_param {
            sched_priority: priority,
        };
        let res = unsafe { libc::sched_setscheduler(0, libc::SCHED_FIFO, &params) };
        if res < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    #[cfg(not(target_os = "linux"))]
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "cannot increase the thread priority on this platform"))
}
