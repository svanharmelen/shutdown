//! # shutdown
//!
//! This crate is meant to be used together with Tokio as it provides an
//! async solution for listening for shutdown signals.
//!
//! When creating a new "root" shutdown channel, it will register itself to
//! listen for SIGINT and SIGTERM signals. When a SIGNINT or SIGTERM is received,
//! it will unregister itself again so any additional signals will be processed
//! as usual (interrupting or terminating the process in most cases). Besides a
//! SIGINT or SIGTERM signal, you can also trigger a shutdown signal manually by
//! calling [shutdown_now](Shutdown::shutdown_now).
//!
//! You can form a tree of branches and subscribers and choose to only shutdown
//! a specific branch. This will shutdown all subscribers but also any child
//! branches and their subscribers. This can be helpful in async applications
//! where lots of tasks spawn lots of tasks, that spawn lots of tasks...

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use log::debug;
use signal_hook::{cleanup, iterator::Signals, SIGINT, SIGTERM};
use tokio::sync::broadcast;

type Sender = Arc<Mutex<Option<broadcast::Sender<()>>>>;
type Receiver = broadcast::Receiver<()>;

#[derive(Debug)]
pub struct Shutdown {
    shutdown: Arc<AtomicBool>,
    signal: Sender,
    notify: Receiver,
}

impl Shutdown {
    /// Create a new shutdown channel. In most cases the channel will be
    /// shutdown when CTRL-C is pressed and the process receives a SIGINT or
    /// SIGTERM signal. If needed you can also call [shutdown_now](Shutdown::shutdown_now)
    /// manually to send a shutdown signal programmatically.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use shutdown::Shutdown;
    ///
    /// let root = Shutdown::new().unwrap();
    /// ```
    pub fn new() -> Result<Self, std::io::Error> {
        let (sender, receiver) = broadcast::channel(1);
        let parent = Arc::new(Mutex::new(Some(sender)));

        // Create the shutdown struct first, as it needs to clone
        // the parent (sender) part of the broadcast channel before
        // moving itself into the signal monitoring thread.
        let shutdown = Self {
            shutdown: Arc::new(AtomicBool::new(false)),
            signal: parent.clone(),
            notify: receiver,
        };

        // Register the SIGINT and SIGTERM signals and their respective
        // cleanup handlers. This makes sure the signals are reset back
        // to their default behavior after receiving the first signal.
        let signals = Signals::new(&[SIGINT, SIGTERM])?;
        cleanup::register(SIGINT, vec![SIGTERM, SIGINT])?;
        cleanup::register(SIGTERM, vec![SIGTERM, SIGINT])?;

        // Spawn a dedicated OS thread for listening for signals.
        std::thread::spawn(move || {
            if let Some(signal) = signals.forever().next() {
                debug!("Received a shutdown signal: {}", signal);
                if let Some(sender) = parent.lock().unwrap().take() {
                    drop(sender);
                }
            }
        });

        Ok(shutdown)
    }

    /// Create a new branch (child) that can be shutdown independent of the
    /// root (parent) channel. When the root (or parent to be more precise)
    /// is shutdown, the new branch (and any child branches) will also be
    /// shutdown.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use shutdown::Shutdown;
    ///
    /// let root = Shutdown::new().unwrap();
    /// let branch = root.branch();
    ///
    /// // Shutdown a specific branch
    /// branch.shutdown_now();
    /// ```
    pub fn branch(&self) -> Self {
        let (sender, receiver) = broadcast::channel(1);
        let child = Arc::new(Mutex::new(Some(sender)));

        // Create a branch that allows shutting down this, and
        // all tasks subscribed or branched from this task.
        let branch = Self {
            shutdown: Arc::new(AtomicBool::new(false)),
            signal: child.clone(),
            notify: receiver,
        };

        match self.signal.lock().unwrap().as_ref() {
            // If our parent is not dropped yet, spawn a task that
            // listens for a shutdown signal and, when received,
            // forwards the signal by dropping the child (signal).
            Some(parent) => {
                let mut notify = parent.subscribe();
                tokio::spawn(async move {
                    let _ = notify.recv().await;
                    if let Some(child) = child.lock().unwrap().take() {
                        drop(child);
                    }
                });
            }
            // If the parent is already dropped, immediately drop
            // the child (signal) to forward the shutdown signal.
            None => {
                drop(child.lock().unwrap().take());
            }
        }

        branch
    }

    /// Create a new subscriber (sibling) that listens to an existing root (or
    /// previously created branch) for any shutdown signals.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use shutdown::Shutdown;
    ///
    /// let root = Shutdown::new().unwrap();
    /// let subscriber = root.subscribe();
    /// ```
    pub fn subscribe(&self) -> Self {
        match self.signal.lock().unwrap().as_ref() {
            Some(sender) => {
                // Return a new shutdown struct that allows creating
                // additional sub-subscribers or branches from it.
                Self {
                    shutdown: self.shutdown.clone(),
                    signal: self.signal.clone(),
                    notify: sender.subscribe(),
                }
            }
            None => {
                // Return a new shutdown struct with a dummy receiver
                // as a shutdown signal was already received.
                let (_, receiver) = broadcast::channel(1);
                Self {
                    shutdown: self.shutdown.clone(),
                    signal: self.signal.clone(),
                    notify: receiver,
                }
            }
        }
    }

    /// Returns `true` is the channel received a shutdown signal.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use shutdown::Shutdown;
    ///
    /// let root = Shutdown::new().unwrap();
    ///
    /// while !root.is_shutdown() {
    ///     // Do stuff...
    /// }
    /// ```
    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    /// Manually shutdown the root or a branch. This causes all connected
    /// subscribers and any child branches to be shutdown as well.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use shutdown::Shutdown;
    ///
    /// let root = Shutdown::new().unwrap();
    /// let branch = root.branch();
    ///
    /// // Trigger a manual shutdown from code
    /// root.shutdown_now();
    /// ```
    pub fn shutdown_now(&self) {
        if let Some(sender) = self.signal.lock().unwrap().take() {
            // Register that a shutdown request was received.
            self.shutdown.store(true, Ordering::SeqCst);
            // Drop the sender to forward the shutdown signal.
            drop(sender);
        }
    }

    /// Block until a shutdown signal is received. This can, for example, be
    /// used in a select to block to wait for a long running task while still
    /// being able to respond to a shutdown signal.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use shutdown::Shutdown;
    /// use tokio::time::{delay_for, Duration};
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let mut root = Shutdown::new().unwrap();
    ///
    ///     tokio::select! {
    ///         _ = root.recv() => (),
    ///         _ = delay_for(Duration::from_secs(300)) => (), // Long runnnig task
    ///     }
    /// }
    /// ```
    pub async fn recv(&mut self) {
        // Return early if a shutdown request has already been received.
        if self.shutdown.load(Ordering::SeqCst) {
            return;
        }

        // Wait until a shutdown signal is received. The possible
        // "lag error" can be ignored as only one value is ever send.
        let _ = self.notify.recv().await;

        // Register that a shutdown request was received.
        self.shutdown.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use libc;
    use tokio::time::{delay_for, Duration};

    #[tokio::test]
    async fn not_notified() {
        let _ = env_logger::Builder::new()
            .format_timestamp(None)
            .filter(None, log::LevelFilter::Debug)
            .is_test(true)
            .try_init();

        let mut root = Shutdown::new().unwrap();
        let branch1 = root.branch();
        let branch2 = branch1.branch();
        let sub1 = branch1.subscribe();
        let sub2 = branch2.subscribe();

        tokio::select! {
            _ = root.recv() => (),
            _ = delay_for(Duration::from_secs(1)) => (),
        }

        assert!(!root.is_shutdown(), "root shutdown without notify");
        assert!(!branch1.is_shutdown(), "branch1 shutdown without notify");
        assert!(!branch2.is_shutdown(), "branch2 shutdown without notify");
        assert!(!sub1.is_shutdown(), "subscriber1 shutdown without notify");
        assert!(!sub2.is_shutdown(), "subscriber2 shutdown without notify");
    }

    #[tokio::test]
    async fn shutdown_sigint() {
        let _ = env_logger::Builder::new()
            .format_timestamp(None)
            .filter(None, log::LevelFilter::Debug)
            .is_test(true)
            .try_init();

        let mut root = Shutdown::new().unwrap();
        let branch1 = root.branch();
        let branch2 = branch1.branch();
        let mut sub1 = branch1.subscribe();
        let mut sub2 = branch2.subscribe();

        unsafe { libc::raise(signal_hook::SIGINT) };

        tokio::select! {
            _ = root.recv() => (),
            _ = delay_for(Duration::from_secs(1)) => (),
        }
        tokio::select! {
            _ = sub1.recv() => (),
            _ = delay_for(Duration::from_secs(1)) => (),
        }
        tokio::select! {
            _ = sub2.recv() => (),
            _ = delay_for(Duration::from_secs(1)) => (),
        }

        assert!(root.is_shutdown(), "root not shutdown (signal)");
        assert!(branch1.is_shutdown(), "branch1 not shutdown (signal)");
        assert!(branch2.is_shutdown(), "branch2 not shutdown (signal)");
        assert!(sub1.is_shutdown(), "subscriber1 not shutdown (signal)");
        assert!(sub2.is_shutdown(), "subscriber2 not shutdown (signal)");
    }

    #[tokio::test]
    async fn shutdown_now() {
        let mut root = Shutdown::new().unwrap();
        let branch1 = root.branch();
        let branch2 = branch1.branch();
        let mut sub1 = branch1.subscribe();
        let mut sub2 = branch2.subscribe();

        root.shutdown_now();

        tokio::select! {
            _ = root.recv() => (),
            _ = delay_for(Duration::from_secs(1)) => (),
        }
        tokio::select! {
            _ = sub1.recv() => (),
            _ = delay_for(Duration::from_secs(1)) => (),
        }
        tokio::select! {
            _ = sub2.recv() => (),
            _ = delay_for(Duration::from_secs(1)) => (),
        }

        assert!(root.is_shutdown(), "root not shutdown (now)");
        assert!(branch1.is_shutdown(), "branch1 not shutdown (now)");
        assert!(branch2.is_shutdown(), "branch2 not shutdown (now)");
        assert!(sub1.is_shutdown(), "subscriber1 not shutdown (now)");
        assert!(sub2.is_shutdown(), "subscriber2 not shutdown (now)");
    }

    #[tokio::test]
    async fn shutdown_branch() {
        let mut root = Shutdown::new().unwrap();
        let branch1 = root.branch();
        let branch2 = branch1.branch();
        let mut sub1 = branch1.subscribe();
        let mut sub2 = branch2.subscribe();

        sub2.shutdown_now();

        tokio::select! {
            _ = root.recv() => (),
            _ = delay_for(Duration::from_secs(1)) => (),
        }
        tokio::select! {
            _ = sub1.recv() => (),
            _ = delay_for(Duration::from_secs(1)) => (),
        }
        tokio::select! {
            _ = sub2.recv() => (),
            _ = delay_for(Duration::from_secs(1)) => (),
        }

        assert!(!root.is_shutdown(), "root shutdown without notify");
        assert!(!branch1.is_shutdown(), "branch1 shutdown without notify");
        assert!(!sub1.is_shutdown(), "subscriber1 shutdown without notify");

        assert!(branch2.is_shutdown(), "branch2 not shutdown (now)");
        assert!(sub2.is_shutdown(), "subscriber2 not shutdown (now)");
    }
}
