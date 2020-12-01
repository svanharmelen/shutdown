# shutdown

shutdown can be used to gracefully exit (part of) a running program

[![Crates.io][crates-badge]][crates-url]
[![Documentation][docs-badge]][docs-url]
[![License][license-badge]][license-url]

[crates-badge]: https://img.shields.io/crates/v/shutdown.svg
[crates-url]: https://crates.io/crates/shutdown
[docs-badge]: https://docs.rs/shutdown/badge.svg
[docs-url]: https://docs.rs/shutdown
[license-badge]: https://img.shields.io/crates/l/shutdown.svg
[license-url]: https://github.com/risoflora/shutdown#license

## Example

The example below shows how to create a new shutdown channel, create a few
branches, subscribe some listeners and shutdown one of the branches:

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

    // Shutdown the first branch.
    branch1.shutdown();
}
```

## Usage

Add shutdown and Tokio to your dependencies:

```toml
shutdown = "0.1"
tokio = { version = "0.2", features = ["full"] }
```

And then get started in your `main.rs`:

```rust
use shutdown::Shutdown;
use tokio::time::{delay_for, Duration};

#[tokio::main]
async fn main() {
    let mut root = Shutdown::new().unwrap();

    while !root.is_shutdown() {
        // Wait for a second before spawning a new task
        tokio::select! {
            _ = root.recv() => break,
            _ = delay_for(Duration::from_secs(1)) => (),
        }

        // Subscribe and spawn a long running task
        let shutdown = root.subscribe();
        tokio::spawn(async move {
            while !shutdown.is_shutdown() {
                // Do stuff until we're shutdown...
            }
        })
    }
}
```

## Running the tests

Because each "root" shutdown channel registers itself to listen for SIGINT and
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
