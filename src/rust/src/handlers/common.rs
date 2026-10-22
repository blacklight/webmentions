//! Shared helpers for the processors — mirrors
//! `webmentions.handlers._common`.

use std::sync::Arc;

use crate::model::Webmention;

/// A mention callback — mirrors the Python `Callable[[Webmention], None]`
/// parameters.
pub type MentionCallback = Arc<dyn Fn(&Webmention) + Send + Sync>;

/// Wrap a mention callback so panics/errors are logged, not propagated —
/// mirrors `on_mention_callback_wrapper`.
///
/// Rust callbacks can't raise like Python exceptions; the wrapper guards
/// against panics so a broken user callback can't kill a delivery thread.
pub fn wrap_callback(callback: Option<MentionCallback>) -> Option<MentionCallback> {
    callback.map(|cb| {
        Arc::new(move |mention: &Webmention| {
            let cb = cb.clone();
            let direction = mention.direction.as_str();
            if let Err(payload) =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cb(mention)))
            {
                let msg = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "unknown panic".to_string());
                log::error!("Error on mention processed callback <direction={direction}>: {msg}");
            }
        }) as MentionCallback
    })
}
