#![warn(missing_docs)]
//!
//! The goal of this library is to allow you to create worker threads, whilst being confident
//! that they will be cleaned up again and you don't "leak" threads.
//!
//! This is achieved by passing a [Signal](struct.Signal.html) object to the newly spawned thread.
//! The thread is responsible for checking this signal for whether it should terminate or not.
//!
//! Therefore this library's concept is not 100% foolproof. Due to the nature of threads, there is
//! no way in Rust to forcibly terminating a thread. So we rely on the thread to be wellbehaved and
//! terminate if asked to do so.
//!
//! ```
//! use managed_thread;
//!
//! // channel to communicate back to main thread
//! let (tx, rx) = std::sync::mpsc::channel::<()>();
//! let owned_thread = managed_thread::spawn_owned(move |signal| {
//!                                 while signal.should_continue() {
//!                                     // do some work
//!                                 }
//!                                 // Send a signal that this thread is exiting
//!                                 tx.send(())
//!                             });
//!
//! // The owned thread will now terminate
//! drop(owned_thread);
//! // confirm that the managed_thread has terminated
//! rx.recv().unwrap();
//! ```

use std::sync::{mpsc, mpsc::TryRecvError};
use std::{thread, thread::JoinHandle};

/// The signal type that is passed to the thread.
///
/// The thread must check the signal periodically for whether it should terminate or not.
/// If the signal notifies the thread to stop, it should exit as fast as possible.
#[derive(Debug)]
pub struct Signal {
    stop_receiver: mpsc::Receiver<()>,
}

impl Signal {
    /// Check whether the thread this signal was passed to is allowed to continue
    ///
    /// Opposite of [should_stop](struct.Signal.html#method.should_stop)
    pub fn should_continue(&self) -> bool {
        !self.should_stop()
    }

    /// Check whether the thread this signal was passed to should stop now.
    /// If this function returns true, the thread should exit as soon as possible.
    ///
    /// Warning: Once this function returned `true`, due to current limitations, it might not return
    /// `true` when called again. Once `true` is returned, exit without checking the signal again.
    pub fn should_stop(&self) -> bool {
        // only if the stream is empty we should continue.
        // Otherwise, either our Controller disappeared, or we received a stop signal
        // => stop the thread
        Err(TryRecvError::Empty) != self.stop_receiver.try_recv()
    }
}

#[derive(Debug)]
struct Controller {
    stop_sender: mpsc::Sender<()>,
}

impl Controller {
    pub fn stop(&self) {
        self.stop_sender.send(()).ok();
    }
}

/// The `OwnedThread` represents a handle to a thread that was spawned using
/// [spawn_owned](fn.spawn_owned.html).
///
/// Whenever the OwnedThread is dropped, the underlying thread is signaled to stop execution.
///
/// Note however that the underlying thread may not exit immediately. It is only guaranted, that
/// the thread will receive the signal to abort, but how long it will keep running depends on
/// the function that is passed when starting the thread.
#[derive(Debug)]
pub struct OwnedThread<T> {
    join_handle: Option<JoinHandle<T>>,
    stop_controller: Controller,
}

impl<T> OwnedThread<T> {
    /// this function is similar to the [`join`](https://doc.rust-lang.org/std/thread/struct.JoinHandle.html#method.join)
    /// function of a [std::JoinHandle](https://doc.rust-lang.org/std/thread/struct.JoinHandle.html#method.join).
    /// When join is called, the thread is signalled to stop and is afterward joined.
    /// Therefore this call is blocking and can return the result from the thread.
    pub fn join(mut self) -> std::thread::Result<T> {
        self.stop();

        // Using the option is necessary because we cannot move out of self, as it implements Drop
        self.join_handle
            .take()
            .expect("joinhandle of OwnedThread does not exist")
            .join()
    }

    /// Signal the underlying thread to stop.
    /// This function is non-blocking.
    pub fn stop(&self) {
        self.stop_controller.stop();
    }
}

/// Drop the OwnedThread.
/// Blocks until the owned thread finishes!
impl<T> Drop for OwnedThread<T> {
    fn drop(&mut self) {
        self.stop();
        if let Some(handle) = self.join_handle.take() {
            handle.join().ok();
        }
    }
}

/// The main function of this library.
///
/// It will create a new thread with a [Signal](struct.Signal.html) that is controlled by the
/// [OwnedThread](struct.OwnedThread.html) object it returns.
///
/// The `thread_function` that is passed to this thread is responsible for periodically checking
/// the signal and to make sure it exits when the signal indicates that it should do so.
pub fn spawn_owned<T: Send + 'static, F: FnOnce(Signal) -> T + Send + 'static>(
    thread_function: F,
) -> OwnedThread<T> {
    let (signal_sender, receiver) = mpsc::channel();
    let signal = Signal {
        stop_receiver: receiver,
    };
    let join_handle = thread::spawn(move || thread_function(signal));
    OwnedThread {
        join_handle: Some(join_handle),
        stop_controller: Controller {
            stop_sender: signal_sender,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::mpsc, thread, time::Duration};
    #[test]
    fn owned_thread_terminates_when_dropped() {
        // channel to communicate back to main thread
        let (tx, rx) = mpsc::channel::<()>();
        let owned_thread = crate::spawn_owned(move |signal| {
            while signal.should_continue() {
                // do some work
            }
            tx.send(())
        });
        thread::sleep(Duration::from_secs(1));
        assert_eq!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));

        // The owned thread will now terminate
        drop(owned_thread);
        // make sure the owned thread has ended
        rx.recv().unwrap();
    }

    #[test]
    fn owned_thread_terminates_when_told_to_stop() {
        // channel to communicate back to main thread
        let (tx, rx) = mpsc::channel::<()>();
        let owned_thread = crate::spawn_owned(move |signal| {
            while signal.should_continue() {
                // do some work
            }
            tx.send(())
        });
        thread::sleep(Duration::from_secs(1));
        assert_eq!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));

        // The owned thread will now terminate
        owned_thread.stop();
        // make sure the owned thread has ended
        rx.recv().unwrap();
    }
}
