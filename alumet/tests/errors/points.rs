use anyhow::anyhow;
use std::sync::Mutex;

pub(crate) static ERRORS: Mutex<Option<PluginErrorPoints>> = Mutex::new(None);
pub(crate) static CATCHES: Mutex<Option<ExpectedCatchPoints>> = Mutex::new(None);

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum Behavior {
    Ok,
    Error,
    Panic,
}

impl Default for Behavior {
    fn default() -> Self {
        Self::Ok
    }
}

/// A place in the program where we can trigger an error or panic.
#[derive(Default)]
pub struct PluginErrorPoints {
    pub name: Behavior,
    pub version: Behavior,
    pub default_config: Behavior,

    pub init: Behavior,
    pub start: Behavior,
    pub stop: Behavior,
    pub post_pipeline_start: Behavior,
    pub drop: Behavior,

    pub source1_build: Behavior,
    pub source2_build: Behavior,
    pub source1_poll: Behavior,
    pub source2_poll: Behavior,

    pub transf_build: Behavior,
    pub transf_apply: Behavior,

    pub output_build: Behavior,
    pub output_write: Behavior,
}

#[derive(PartialEq, Eq, Debug)]
pub enum Expect {
    None,
    Error,
    Panic,
    // Log, // TODO check that logs are emitted at some points
}

impl Default for Expect {
    fn default() -> Self {
        Self::None
    }
}

/// A place in the program where we expect errors and panics to be detected.
#[derive(Default)]
pub struct ExpectedCatchPoints {
    pub init: Expect,
    pub agent_build_and_start: Expect,
    pub agent_default_config: Expect,
    #[allow(unused)]
    pub runtime: Expect,
    pub shutdown: Expect,
    pub wait_for_shutdown: Expect,
}

macro_rules! panic_point {
    ($point:ident) => {
        use crate::errors::points as p;
        let should_panic = {
            // Make sure to release the lock BEFORE panicking, otherwise it will poison the lock and
            // cause a non-unwinding panic if someone else attempts to use it (such as `panic_point!` in `BadPlugin::drop`)
            p::ERRORS.lock().expect("failed to acquire lock").as_ref().unwrap().$point == p::Behavior::Panic
        };
        if should_panic {
            panic!("test panic triggered on point {}", stringify!($point))
        }
    };
}

macro_rules! error_point {
    ($point:ident) => {
        use crate::errors::points as p;
        let behavior = {
            p::ERRORS
                .lock()
                .expect("failed to acquire lock")
                .as_ref()
                .unwrap()
                .$point
        };
        match behavior {
            p::Behavior::Ok => (),
            p::Behavior::Panic => panic!("test panic triggered on point {}", stringify!($point)),
            p::Behavior::Error => Err(anyhow::anyhow!("test error triggered on point {}", stringify!($point)))?,
        }
    };
}

pub enum CatchResult<T> {
    Ok(T),
    Panic(Box<dyn std::any::Any + Send + 'static>),
    Err(anyhow::Error),
}

pub enum PointCheckResult<T> {
    Ok(T),
    ExpectedErrorOrPanic(()),
    UnexpectedErrorOrPanic(anyhow::Error),
}

impl<T> From<std::thread::Result<anyhow::Result<T>>> for CatchResult<T> {
    fn from(value: std::thread::Result<anyhow::Result<T>>) -> Self {
        match value {
            Ok(Ok(res)) => CatchResult::Ok(res),
            Ok(Err(normal_err)) => CatchResult::Err(normal_err),
            Err(panic) => CatchResult::Panic(panic),
        }
    }
}

macro_rules! handle_check_res {
    ($res:expr) => {
        match $res {
            p::PointCheckResult::Ok(value) => value,
            p::PointCheckResult::ExpectedErrorOrPanic(_) => return Ok(()),
            p::PointCheckResult::UnexpectedErrorOrPanic(e) => return Err(e),
        }
    };
}

macro_rules! catch_panic_point {
    ($point:ident, $faillible:expr) => {{
        use crate::errors::points as p;
        use std::panic::AssertUnwindSafe;

        let lock = p::CATCHES.lock().unwrap();
        let expected = &lock.as_ref().unwrap().$point;
        let actual = p::catch_unwind_silent(AssertUnwindSafe($faillible));
        let actual = actual.map(|r| Ok(r));

        let res = p::process_catch_point(stringify!($point), expected, actual.into());
        p::handle_check_res!(res)
    }};
}

macro_rules! catch_error_point {
    ($point:ident, $faillible:expr) => {{
        use crate::errors::points as p;
        use std::panic::AssertUnwindSafe;

        let lock = p::CATCHES.lock().unwrap();
        let expected = &lock.as_ref().unwrap().$point;
        let actual = p::catch_unwind_silent(AssertUnwindSafe($faillible));

        let res = p::process_catch_point(stringify!($point), expected, actual.into());
        p::handle_check_res!(res)
    }};
}

pub fn set_error_points(points: PluginErrorPoints) {
    let mut lock = ERRORS.lock().unwrap();
    *lock = Some(points);
}

pub fn set_expected_catches(expected: ExpectedCatchPoints) {
    let mut lock = CATCHES.lock().unwrap();
    *lock = Some(expected);
}

pub fn process_catch_point<R>(name: &str, expected: &Expect, actual: CatchResult<R>) -> PointCheckResult<R> {
    match (expected, actual) {
        (Expect::None, CatchResult::Ok(res)) => PointCheckResult::Ok(res),
        (Expect::None, CatchResult::Panic(e)) => {
            PointCheckResult::UnexpectedErrorOrPanic(anyhow!("unexpected panic at {name}: {e:?}"))
        }
        (Expect::None, CatchResult::Err(e)) => {
            PointCheckResult::UnexpectedErrorOrPanic(anyhow!("unexpected error at {name}: {e:?}"))
        }
        (Expect::Error, CatchResult::Ok(_)) => {
            PointCheckResult::UnexpectedErrorOrPanic(anyhow!("expected error at {name}, got Ok(...)"))
        }
        (Expect::Error, CatchResult::Panic(e)) => PointCheckResult::UnexpectedErrorOrPanic(anyhow!(
            "expected error at {name}, got panic (see backtrace below): {e:?}"
        )),
        (Expect::Error, CatchResult::Err(_)) => PointCheckResult::ExpectedErrorOrPanic(()),
        (Expect::Panic, CatchResult::Ok(_)) => {
            PointCheckResult::UnexpectedErrorOrPanic(anyhow!("expected panic at {name}, got Ok(...)"))
        }
        (Expect::Panic, CatchResult::Panic(_)) => PointCheckResult::ExpectedErrorOrPanic(()),
        (Expect::Panic, CatchResult::Err(e)) => {
            PointCheckResult::UnexpectedErrorOrPanic(anyhow!("expected panic at {name}, got error: {e}"))
        }
    }
}

pub fn catch_unwind_silent<F: FnOnce() -> R + std::panic::UnwindSafe, R>(f: F) -> std::thread::Result<R> {
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(f);
    std::panic::set_hook(prev_hook);
    result
}

pub(crate) use catch_error_point;
pub(crate) use catch_panic_point;
pub(crate) use handle_check_res;

pub(crate) use error_point;
pub(crate) use panic_point;
