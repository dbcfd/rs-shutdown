///! Shutdown
///!
///! Provides a signal handler which by default will listen to shutdown type signals
///! (e.g. SIGINT, SIGHUP, SIGTERM, SIGQUIT) and provide either a future that completes with the
///! signal or invoke a callback to provide you with the result of watching for signals.
///!
pub extern crate futures;
#[macro_use(debug, log)] pub extern crate log;
pub extern crate tokio;
pub extern crate tokio_signal;

pub mod prelude {
    pub use futures;
    pub use tokio;
    pub use tokio_signal;
    pub use tokio_signal::unix;
    pub use tokio_signal::unix::Signal;
    pub use tokio_signal::unix::libc;
}

use prelude::*;
use futures::{Future, Stream};
use futures::future::Either;
use std::io::{Error as IOError};

///
/// Default signal set
///
pub const DEFAULT_SIGNALS: [unix::libc::c_int; 3] = [
    unix::SIGHUP,
    unix::SIGQUIT,
    unix::SIGTERM
];

///
/// SignalHandler provides implementations to convert a set of watched signals to a single future,
/// or to provide a way to watch a set of signals and invoke a callback.
///
/// # Examples
///
/// ```text
/// let signal_handler = SignalHandler::new();
///
/// tokio::executor::current_thread::block_on_all(signal_handler).expect("Could not receive signal");
///
/// println!("Ctrl-c received");
/// ```
///
pub struct SignalHandler;

impl SignalHandler {
    ///
    /// Set a function to be invoked when ctrl-c or the default signal set `DEFAULT_SIGNALS` is
    /// invoked.
    ///
    /// # Arguments
    ///
    ///  * `func` - function to be invoked when the default signal `DEFAULT_SIGNALS` set is captured.
    ///
    pub fn watch<F>(func: F) -> Box<Future<Item=(), Error=IOError> + Send>
        where F: FnOnce(libc::c_int) -> () + 'static + Send {
        SignalHandler::watch_signals(&DEFAULT_SIGNALS, func)
    }

    ///
    /// Set a function to be invoked when either ctrl-c or a set of `signals` is invoked.
    ///
    /// # Arguments
    ///
    ///  * `signals` - array of signals to be monitored in addition to ctrl-c
    ///  * `func` - function to be invoked when ctrl-c or one of `signals` is encountered
    ///
    pub fn watch_signals<F>(signals: &[libc::c_int], func: F) -> Box<Future<Item=(), Error=IOError> + Send>
        where F: FnOnce(libc::c_int) -> () + 'static + Send {
        let fut_res = SignalHandler::new_with_signals(signals)
            .map(|r| {
                debug!("Passing signal {:?} to function", r);
                func(r);
            });

        Box::new(fut_res)
    }

    ///
    /// Create a future that will complete when ctrl-c or the default signal set `DEFAULT_SIGNALS`
    /// is captured.
    ///
    pub fn new() -> Box<Future<Item=libc::c_int, Error=IOError> + Send> {
        SignalHandler::new_with_signals(&DEFAULT_SIGNALS)
    }

    ///
    /// Create a future that will complete when ctrl-c or the default signal set `DEFAULT_SIGNALS`
    /// is captured.
    ///
    pub fn new_with_signals(signals: &[libc::c_int]) -> Box<Future<Item=libc::c_int, Error=IOError> + Send> {
        let sig_int: Box<Future<Item=libc::c_int, Error=IOError> + Send> = Box::new(
            tokio_signal::ctrl_c()
                .flatten_stream()
                .take(1)
                .collect()
                .map(|_| {
                    debug!("ctrl-c received");
                    libc::SIGINT
                })
        );

        signals.iter().fold(sig_int, |f, s| {
            let signal = s.clone();
            let other_fut = tokio_signal::unix::Signal::new(signal)
                .flatten_stream()
                .take(1)
                .collect()
                .map(move |_| {
                    signal
                });

            let combined_fut = f.select2(other_fut).then(|j| {
                // futures::Either will have a return in the form of (value/error of first completed future,
                // outstanding future). We only care about the first value received, so in both
                // the ok case and the error case, we take the value or error seen, and ignore
                // the outstanding future
                match j {
                    Ok(Either::A((v, _))) => Ok(v),
                    Ok(Either::B((v, _))) => Ok(v),
                    Err(Either::A((e, _))) => Err(e),
                    Err(Either::B((e, _))) => Err(e)
                }
            });
            Box::new(combined_fut)
        })
    }
}

#[cfg(test)]
mod tests {
    extern crate env_logger;
    extern crate nix;

    use super::*;

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn raise_signal() {
        assert!(nix::sys::signal::raise(nix::sys::signal::SIGUSR2).is_ok());
    }

    #[test]
    fn test_shutdown() {
        let _ = env_logger::try_init();

        let signal_raised = std::sync::Arc::new(std::sync::Mutex::new(0));

        let child = std::thread::spawn({
            let thread_signal_raised = signal_raised.clone();
            move || {
                let signals = [unix::SIGHUP, unix::SIGUSR2];

                let signal_handler = SignalHandler::new_with_signals(&signals);

                let sig = tokio::executor::current_thread::block_on_all(signal_handler);

                if let Ok(mut current_signal_raised) = thread_signal_raised.lock() {
                    if *current_signal_raised == 1 {
                        *current_signal_raised = 2;
                    }
                }

                sig
            }
        });

        std::thread::sleep(std::time::Duration::from_secs(5));

        if let Ok(mut current_signal_raised) = signal_raised.lock() {
            *current_signal_raised = 1;
        }
        raise_signal();

        let child_res = child.join().expect("Could not join child");

        let res = child_res.expect("Failed to produce signal");

        assert_eq!(res, libc::SIGUSR2);

        assert_eq!(*signal_raised.lock().unwrap(), 2);
    }

    #[test]
    fn test_watch() {
        let _ = env_logger::try_init();

        let mut rt = tokio::runtime::Runtime::new().expect("No runtime");

        let (tx, rx) = futures::sync::oneshot::channel();

        let signals = [libc::SIGUSR2, libc::SIGHUP];

        let signal_future = SignalHandler::watch_signals(&signals, move |signal| {
            debug!("Received signal {:?} when watching", signal);

            tx.send(signal).ok().expect("Failed to send signal");
        }).map_err(|_| ());

        rt.spawn(signal_future);

        std::thread::sleep(std::time::Duration::from_secs(5));

        raise_signal();

        let signal_seen = rx.wait().expect("Did not receive signal");

        assert_eq!(signal_seen, libc::SIGUSR2);

        rt.shutdown_now().wait().expect("Failed to shutdown");


    }
}