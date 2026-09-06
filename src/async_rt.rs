//! A single, lazily-initialized Tokio runtime shared by every part of this
//! plugin that needs to call into `time-tracking-cli`'s async API
//! (`DataService`, `create_template_content`, ...).
//!
//! `time-tracking-cli` is built with `default-features = false`, but `tokio`
//! is a hard, non-optional dependency of that crate regardless of features,
//! and so are the async functions this plugin calls. A single current-thread
//! runtime, reused via `block_on`, is enough: every caller here is
//! synchronous Neovim command-handler code, invoked on purpose by the user,
//! doing only local-disk work.

use std::sync::OnceLock;
use tokio::runtime::Runtime;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build the time-tracking-nvim async runtime")
    })
}

/// Run `fut` to completion on the shared runtime, blocking the calling
/// thread until it finishes.
pub fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    runtime().block_on(fut)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_on_runs_an_async_block_to_completion() {
        let result = block_on(async { 2 + 2 });
        assert_eq!(result, 4);
    }

    #[test]
    fn block_on_reuses_the_same_runtime_across_calls() {
        let a = block_on(async { 1 });
        let b = block_on(async { 2 });
        assert_eq!((a, b), (1, 2));
    }
}
