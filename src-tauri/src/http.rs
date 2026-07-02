//! Shared `reqwest` client.
//!
//! Every outbound HTTP call (STT and formatter) goes through one lazily-built
//! client. `reqwest::Client` is internally an `Arc`, so [`client`] hands out a
//! cheap clone of the same connection pool rather than rebuilding TLS state and
//! sockets per request. The client is created once, on first use, and cached
//! in a process-wide [`OnceLock`].

use std::sync::OnceLock;
use std::time::Duration;

/// Request timeout applied to every call. Matches the 20s the formatter used
/// before the client was shared.
const TIMEOUT: Duration = Duration::from_secs(20);

static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// Counts how many times the client has actually been built. Under a correct
/// [`OnceLock`] this only ever reaches 1, which the tests assert.
#[cfg(test)]
static BUILD_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn build() -> reqwest::Client {
    #[cfg(test)]
    BUILD_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .expect("failed to build http client")
}

/// Returns a clone of the shared client, building it on first use. The clone is
/// cheap: it shares the underlying connection pool with every other clone.
pub fn client() -> reqwest::Client {
    CLIENT.get_or_init(build).clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn client_is_built_once_and_reused() {
        // Two calls must not rebuild: the OnceLock initializes exactly once.
        let a = client();
        let _b = client();
        // `a` is a live clone; drop is a no-op for the shared pool.
        drop(a);
        assert_eq!(
            BUILD_COUNT.load(Ordering::SeqCst),
            1,
            "client must be built exactly once across repeated calls"
        );
    }
}
