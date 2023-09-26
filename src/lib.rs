//! # shutdown
//!
//! This crate is meant to be used together with Tokio as it provides an
//! async solution for listening and forwarding shutdown signals.
//!
//! When creating a new "root" shutdown signal, it will register itself to
//! listen for SIGINT and SIGTERM signals. When a SIGNINT or SIGTERM is received,
//! it will unregister itself again so any additional signals will be processed
//! as usual (interrupting or terminating the process in most cases). Besides a
//! SIGINT or SIGTERM signal, you can also trigger a shutdown signal manually by
//! calling [signal](Shutdown::signal).
//!
//! You can form a tree of branches and subscribers and choose to only shutdown
//! a specific branch. This will shutdown all subscribers but also any child
//! branches and their subscribers. This can be helpful in async applications
//! where lots of tasks spawn lots of tasks, that spawn lots of tasks...

use std::sync::{atomic::AtomicBool, Arc};

use futures::stream::StreamExt;
use log::debug;
use signal_hook::{
    consts::{SIGINT, SIGTERM},
    flag,
};
use signal_hook_tokio::Signals;
use tokio_util::sync::CancellationToken;

pub struct Shutdown {
    token: CancellationToken,
}

impl Clone for Shutdown {
    fn clone(&self) -> Self {
        self.subscribe()
    }
}

impl Shutdown {
    /// Create a new shutdown signal. In most cases the signal will be
    /// triggered when CTRL-C is pressed and the process receives a SIGINT or
    /// SIGTERM signal. If needed you can also call [signal](Shutdown::signal)
    /// to send a shutdown signal programmatically.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use shutdown::Shutdown;
    ///
    /// let root = Shutdown::new().unwrap();
    /// ```
    pub fn new() -> Result<Self, std::io::Error> {
        // Create a cancellation token.
        let token = CancellationToken::new();

        // Create a new shutdown signal.
        let shutdown = Self {
            token: token.clone(),
        };

        // Register the SIGINT and SIGTERM signals.
        let mut signals = Signals::new([SIGINT, SIGTERM])?;

        // Spawn a Tokio task that will listen for signals.
        tokio::spawn(async move {
            if let Some(signal) = signals.next().await {
                debug!("Received a shutdown signal: {}", signal);
                // Register conditional shutdown handlers. This makes sure the
                // application will terminate after receiving a second signal.
                flag::register_conditional_shutdown(SIGINT, 0, Arc::new(AtomicBool::new(true)))
                    .unwrap();
                flag::register_conditional_shutdown(SIGTERM, 0, Arc::new(AtomicBool::new(true)))
                    .unwrap();
                // Send the shutdown signal by cancelling the token.
                token.cancel();
            }
        });

        Ok(shutdown)
    }

    /// Create a new branch (child) that can be signalled independent of the
    /// root (parent). When the root (or parent to be more precise) is signalled,
    /// the new branch (and any child branches) will also be signalled.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use shutdown::Shutdown;
    ///
    /// let root = Shutdown::new().unwrap();
    /// let branch = root.branch();
    ///
    /// // Signal a specific branch
    /// branch.signal();
    /// ```
    pub fn branch(&self) -> Self {
        Self {
            token: self.token.child_token(),
        }
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
        Self {
            token: self.token.clone(),
        }
    }

    /// Returns `true` if a shutdown signal has been received.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use shutdown::Shutdown;
    ///
    /// let root = Shutdown::new().unwrap();
    ///
    /// while !root.is_signalled() {
    ///     // Do stuff...
    /// }
    /// ```
    pub fn is_signalled(&self) -> bool {
        self.token.is_cancelled()
    }

    /// Manually signal the root or a branch. This causes all connected
    /// subscribers and any child branches to be signalled as well.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use shutdown::Shutdown;
    ///
    /// let root = Shutdown::new().unwrap();
    /// let branch = root.branch();
    ///
    /// // Trigger a signal from code
    /// root.signal();
    /// ```
    pub fn signal(&self) {
        self.token.cancel();
    }

    /// Block until a shutdown signal is received. This can, for example, be
    /// used in a select to block to wait for a long running task while still
    /// being able to respond to a shutdown signal.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use shutdown::Shutdown;
    /// use tokio::time::{sleep, Duration};
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let mut root = Shutdown::new().unwrap();
    ///
    ///     tokio::select! {
    ///         _ = root.signalled() => (),
    ///         _ = sleep(Duration::from_secs(300)) => (), // Long runnnig task
    ///     }
    /// }
    /// ```
    pub async fn signalled(&self) {
        self.token.cancelled().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn not_notified() {
        let _ = env_logger::Builder::new()
            .format_timestamp(None)
            .filter(None, log::LevelFilter::Debug)
            .is_test(true)
            .try_init();

        let root = Shutdown::new().unwrap();
        let branch1 = root.branch();
        let branch2 = branch1.branch();
        let sub1 = branch1.subscribe();
        let sub2 = branch2.subscribe();

        tokio::select! {
            _ = root.signalled() => (),
            _ = sleep(Duration::from_secs(1)) => (),
        }

        assert!(!root.is_signalled(), "root shutdown without notify");
        assert!(!branch1.is_signalled(), "branch1 shutdown without notify");
        assert!(!branch2.is_signalled(), "branch2 shutdown without notify");
        assert!(!sub1.is_signalled(), "subscriber1 shutdown without notify");
        assert!(!sub2.is_signalled(), "subscriber2 shutdown without notify");
    }

    #[tokio::test]
    async fn shutdown_sigint() {
        let _ = env_logger::Builder::new()
            .format_timestamp(None)
            .filter(None, log::LevelFilter::Debug)
            .is_test(true)
            .try_init();

        let root = Shutdown::new().unwrap();
        let branch1 = root.branch();
        let branch2 = branch1.branch();
        let sub1 = branch1.subscribe();
        let sub2 = branch2.subscribe();

        unsafe { libc::raise(signal_hook::consts::SIGINT) };

        tokio::select! {
            _ = root.signalled() => (),
            _ = sleep(Duration::from_secs(1)) => (),
        }
        tokio::select! {
            _ = sub1.signalled() => (),
            _ = sleep(Duration::from_secs(1)) => (),
        }
        tokio::select! {
            _ = sub2.signalled() => (),
            _ = sleep(Duration::from_secs(1)) => (),
        }

        assert!(root.is_signalled(), "root not shutdown (signal)");
        assert!(branch1.is_signalled(), "branch1 not shutdown (signal)");
        assert!(branch2.is_signalled(), "branch2 not shutdown (signal)");
        assert!(sub1.is_signalled(), "subscriber1 not shutdown (signal)");
        assert!(sub2.is_signalled(), "subscriber2 not shutdown (signal)");
    }

    #[tokio::test]
    async fn shutdown_now() {
        let root = Shutdown::new().unwrap();
        let branch1 = root.branch();
        let branch2 = branch1.branch();
        let sub1 = branch1.subscribe();
        let sub2 = branch2.subscribe();

        root.signal();

        tokio::select! {
            _ = root.signalled() => (),
            _ = sleep(Duration::from_secs(1)) => (),
        }
        tokio::select! {
            _ = sub1.signalled() => (),
            _ = sleep(Duration::from_secs(1)) => (),
        }
        tokio::select! {
            _ = sub2.signalled() => (),
            _ = sleep(Duration::from_secs(1)) => (),
        }

        assert!(root.is_signalled(), "root not shutdown (manual)");
        assert!(branch1.is_signalled(), "branch1 not shutdown (manual)");
        assert!(branch2.is_signalled(), "branch2 not shutdown (manual)");
        assert!(sub1.is_signalled(), "subscriber1 not shutdown (manual)");
        assert!(sub2.is_signalled(), "subscriber2 not shutdown (manual)");
    }

    #[tokio::test]
    async fn shutdown_branch() {
        let root = Shutdown::new().unwrap();
        let branch1 = root.branch();
        let branch2 = branch1.branch();
        let sub1 = branch1.subscribe();
        let sub2 = branch2.subscribe();

        sub2.signal();

        tokio::select! {
            _ = root.signalled() => (),
            _ = sleep(Duration::from_secs(1)) => (),
        }
        tokio::select! {
            _ = sub1.signalled() => (),
            _ = sleep(Duration::from_secs(1)) => (),
        }
        tokio::select! {
            _ = sub2.signalled() => (),
            _ = sleep(Duration::from_secs(1)) => (),
        }

        assert!(!root.is_signalled(), "root shutdown without notify");
        assert!(!branch1.is_signalled(), "branch1 shutdown without notify");
        assert!(!sub1.is_signalled(), "subscriber1 shutdown without notify");

        assert!(branch2.is_signalled(), "branch2 not shutdown (manual)");
        assert!(sub2.is_signalled(), "subscriber2 not shutdown (manual)");
    }
}
