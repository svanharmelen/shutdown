# shutdown

shutdown can be used to gracefully exit (part of) a running program

[![Build Status][gh-actions-badge]][gh-actions-url]
[![Crates.io][crates-badge]][crates-url]
[![Documentation][docs-badge]][docs-url]
[![License][license-badge]][license-url]

[gh-actions-badge]: https://github.com/svanharmelen/shutdown/workflows/Test%20and%20Build/badge.svg
[gh-actions-url]: https://github.com/svanharmelen/shutdown/actions?workflow=Test%20and%20Build
[crates-badge]: https://img.shields.io/crates/v/shutdown.svg
[crates-url]: https://crates.io/crates/shutdown
[docs-badge]: https://docs.rs/shutdown/badge.svg
[docs-url]: https://docs.rs/shutdown
[license-badge]: https://img.shields.io/crates/l/shutdown.svg
[license-url]: https://github.com/svanharmelen/shutdown#license

## Example

The example below shows how to create a new shutdown signal, create a few
branches, subscribe some listeners and signal one of the branches:

```rust
use shutdown::Shutdown;

fn main() {
    let root = Shutdown::new().unwrap();

    // Create two new branches.
    let branch1 = root.branch();
    let branch2 = root.branch();

    // Create two new subscribers to the first branch.
    let subscriber1 = branch1.subscribe();
    let subscriber2 = branch1.subscribe();

    // Signal the first branch.
    branch1.signal();
}
```

## Usage

Add shutdown and Tokio to your dependencies:

```toml
shutdown = "0.3"
tokio = { version = "1", features = ["full"] }
```

And then get started in your `main.rs`:

```rust
use shutdown::Shutdown;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() {
    let mut root = Shutdown::new().unwrap();

    while !root.is_signalled() {
        // Wait for a task to finish while also
        // listening for any shutdown signals.
        tokio::select! {
            _ = sleep(Duration::from_secs(30)) => (),
            _ = root.received() => break,
        }

        // Subscribe and spawn a long running task which will
        // end its loop when a shutdown signal is received.
        let shutdown = root.subscribe();
        tokio::spawn(async move {
            while !shutdown.is_signalled() {
                // Do stuff until we're shutdown...
            }
        })
    }
}
```

## Running the tests

Because each "root" shutdown signal registers itself to listen for SIGINT and
SIGTERM signals, the test need to run one by one. So to run the tests, please
execute:

```sh
$ cargo test -- --test-threads=1
```

## Contributions

Pull requests and issues are always welcome and appreciated!

## License

shutdown is distributed under the terms of both the MIT license and the Apache License (Version 2.0)

- Apache License 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
