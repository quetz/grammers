// Copyright 2020 - developers of the `grammers` project.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Methods to deal with and offer access to updates.

use super::Client;
use crate::types::{ChatMap, Update};
use futures_util::future::{select, Either};
pub use grammers_mtsender::{AuthorizationError, InvocationError};
use grammers_session::{channel_id, UpdateResult};
pub use grammers_session::{PrematureEndReason, UpdateState};
use grammers_tl_types as tl;
use std::pin::pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep_until;

/// How long to wait after warning the user that the updates limit was exceeded.
const UPDATE_LIMIT_EXCEEDED_LOG_COOLDOWN: Duration = Duration::from_secs(300);

impl Client {
    /// Returns the next update from the buffer where they are queued until used.
    ///
    /// # Example
    ///
    /// ```
    /// # async fn f(client: grammers_client::Client) -> Result<(), Box<dyn std::error::Error>> {
    /// use grammers_client::Update;
    ///
    /// loop {
    ///     let update = client.next_update().await?;
    ///     // Echo incoming messages and ignore everything else
    ///     match update {
    ///         Update::NewMessage(mut message) if !message.outgoing() => {
    ///             message.respond(message.text()).await?;
    ///         }
    ///         _ => {}
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn next_update(&self) -> Result<Update, InvocationError> {
        loop {
            let (update, chats) = self.next_raw_update().await?;

            if let Some(update) = Update::new(self, update, &chats) {
                return Ok(update);
            }
        }
    }

    /// Returns the next raw update and associated chat map from the buffer where they are queued until used.
    ///
    /// # Example
    ///
    /// ```
    /// # async fn f(client: grammers_client::Client) -> Result<(), Box<dyn std::error::Error>> {
    /// loop {
    ///     let (update, chats) = client.next_raw_update().await?;
    ///
    ///     // Print all incoming updates in their raw form
    ///     dbg!(update);
    /// }
    /// # Ok(())
    /// # }
    ///
    /// ```
    ///
    /// P.S. If you don't receive updateBotInlineSend, go to [@BotFather](https://t.me/BotFather), select your bot and click "Bot Settings", then "Inline Feedback" and select probability.
    ///
    pub async fn next_raw_update(
        &self,
    ) -> Result<(tl::enums::Update, Arc<ChatMap>), InvocationError> {
        loop {
            let (deadline, get_diff, get_channel_diff) = {
                let state = &mut *self.0.state.write().unwrap();
                if let Some(update) = state.updates.pop_front() {
                    return Ok(update);
                }
                (
                    state.message_box.check_deadlines(), // first, as it might trigger differences
                    state.message_box.get_difference(),
                    state.message_box.get_channel_difference(&state.chat_hashes),
                )
            };

            if let Some(request) = get_diff {
                let response = self.invoke(&request).await?;
                let (updates, users, chats) = {
                    let state = &mut *self.0.state.write().unwrap();
                    state
                        .message_box
                        .apply_difference(response, &mut state.chat_hashes)
                };
                self.extend_update_queue(updates, ChatMap::new(users, chats));
                continue;
            }

            if let Some(request) = get_channel_diff {
                let maybe_response = self.invoke(&request).await;

                let response = match maybe_response {
                    Ok(r) => r,
                    Err(e) if e.is("PERSISTENT_TIMESTAMP_OUTDATED") => {
                        // According to Telegram's docs:
                        // "Channel internal replication issues, try again later (treat this like an RPC_CALL_FAIL)."
                        // We can treat this as "empty difference" and not update the local pts.
                        // Then this same call will be retried when another gap is detected or timeout expires.
                        //
                        // Another option would be to literally treat this like an RPC_CALL_FAIL and retry after a few
                        // seconds, but if Telegram is having issues it's probably best to wait for it to send another
                        // update (hinting it may be okay now) and retry then.
                        //
                        // This is a bit hacky because MessageBox doesn't really have a way to "not update" the pts.
                        // Instead we manually extract the previously-known pts and use that.
                        log::warn!("Getting difference for channel updates caused PersistentTimestampOutdated; ending getting difference prematurely until server issues are resolved");
                        {
                            self.0
                                .state
                                .write()
                                .unwrap()
                                .message_box
                                .end_channel_difference(
                                    &request,
                                    PrematureEndReason::TemporaryServerIssues,
                                );
                        }
                        continue;
                    }
                    Err(e) if e.is("CHANNEL_PRIVATE") => {
                        log::info!(
                            "Account is now banned in {} so we can no longer fetch updates from it",
                            channel_id(&request)
                                .map(|i| i.to_string())
                                .unwrap_or_else(|| "empty channel".into())
                        );
                        {
                            self.0
                                .state
                                .write()
                                .unwrap()
                                .message_box
                                .end_channel_difference(&request, PrematureEndReason::Banned);
                        }
                        continue;
                    }
                    Err(InvocationError::Rpc(rpc_error)) if rpc_error.code == 500 => {
                        log::warn!("Telegram is having internal issues: {:#?}", rpc_error);
                        {
                            self.0
                                .state
                                .write()
                                .unwrap()
                                .message_box
                                .end_channel_difference(
                                    &request,
                                    PrematureEndReason::TemporaryServerIssues,
                                );
                        }
                        continue;
                    }
                    Err(e) => return Err(e),
                };

                let (updates, users, chats) = {
                    let state = &mut *self.0.state.write().unwrap();
                    state.message_box.apply_channel_difference(
                        request,
                        response,
                        &mut state.chat_hashes,
                    )
                };

                self.extend_update_queue(updates, ChatMap::new(users, chats));
                continue;
            }

            let sleep = pin!(async { sleep_until(deadline.into()).await });
            let step = pin!(async { self.step().await });

            match select(sleep, step).await {
                Either::Left(_) => {}
                Either::Right((step, _)) => step?,
            }
        }
    }

    pub(crate) fn process_socket_updates(&self, all_updates: Vec<tl::enums::Updates>) {
        if all_updates.is_empty() {
            return;
        }

        let result = {
            let state = &mut *self.0.state.write().unwrap();
            let mut res = UpdateResult::default();

            for updates in all_updates {
                if state
                    .message_box
                    .ensure_known_peer_hashes(&updates, &mut state.chat_hashes)
                    .is_err()
                {
                    continue;
                }
                match state
                    .message_box
                    .process_updates(updates, &state.chat_hashes)
                {
                    Ok(UpdateResult {
                        updates,
                        users,
                        chats,
                    }) => {
                        res.updates.extend(updates);
                        res.users.extend(users);
                        res.chats.extend(chats);
                    }
                    Err(_) => return,
                }
            }
            res
        };

        if !result.updates.is_empty() {
            self.extend_update_queue(result.updates, ChatMap::new(result.users, result.chats));
        }
    }

    fn extend_update_queue(&self, mut updates: Vec<tl::enums::Update>, chat_map: Arc<ChatMap>) {
        let mut state = self.0.state.write().unwrap();

        if let Some(limit) = self.0.config.params.update_queue_limit {
            if let Some(exceeds) = (state.updates.len() + updates.len()).checked_sub(limit + 1) {
                let exceeds = exceeds + 1;
                let now = Instant::now();
                let notify = match state.last_update_limit_warn {
                    None => true,
                    Some(instant) => now - instant > UPDATE_LIMIT_EXCEEDED_LOG_COOLDOWN,
                };

                updates.truncate(updates.len() - exceeds);
                if notify {
                    log::warn!(
                        "{} updates were dropped because the update_queue_limit was exceeded",
                        exceeds
                    );
                }

                state.last_update_limit_warn = Some(now);
            }
        }

        state
            .updates
            .extend(updates.into_iter().map(|u| (u, chat_map.clone())));
    }

    /// Synchronize the updates state to the session.
    pub fn sync_update_state(&self) {
        let state = self.0.state.read().unwrap();
        self.0
            .config
            .session
            .set_state(state.message_box.session_state());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::future::Future;

    fn get_client() -> Client {
        panic!()
    }

    #[test]
    fn ensure_next_update_future_impls_send() {
        if false {
            // We just want it to type-check, not actually run.
            fn typeck(_: impl Future + Send) {}
            typeck(get_client().next_update());
        }
    }
}
