// Copyright 2020 - developers of the `grammers` project.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.
use std::sync::Arc;

use grammers_tl_types as tl;

use super::{CallbackQuery, ChatMap, InlineQuery, Message};
use crate::{types::MessageDeletion, Client};

#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum Update {
    /// Occurs whenever a new text message or a message with media is produced.
    NewMessage(Message),
    /// Occurs when a message is updated.
    MessageEdited(Message),
    /// Occurs when a message is deleted.
    MessageDeleted(MessageDeletion),
    /// Occurs when Telegram calls back into your bot because an inline callback
    /// button was pressed.
    CallbackQuery(CallbackQuery),
    /// Occurs whenever you sign in as a bot and a user sends an inline query
    /// such as `@bot query`.
    InlineQuery(InlineQuery),
    /// Raw events are not actual events.
    /// Instead, they are the raw Update object that Telegram sends. You
    /// normally shouldn’t need these.
    ///
    /// **NOTE**: the library can split raw updates into actual `Update`
    /// variants so use this only as the workaround when such variant is not
    /// available yet.
    Raw(Box<tl::enums::Update>),
}

impl Update {
    pub(crate) fn new(
        client: &Client,
        update: tl::enums::Update,
        chats: &Arc<ChatMap>,
    ) -> Option<Self> {
        match update {
            // NewMessage
            tl::enums::Update::NewMessage(tl::types::UpdateNewMessage { message, .. }) => {
                Message::new(client, message, chats).map(Self::NewMessage)
            }
            tl::enums::Update::NewChannelMessage(tl::types::UpdateNewChannelMessage {
                message,
                ..
            }) => Message::new(client, message, chats).map(Self::NewMessage),

            // MessageEdited
            tl::enums::Update::EditMessage(tl::types::UpdateEditMessage { message, .. }) => {
                Message::new(client, message, chats).map(Self::MessageEdited)
            }
            tl::enums::Update::EditChannelMessage(tl::types::UpdateEditChannelMessage {
                message,
                ..
            }) => Message::new(client, message, chats).map(Self::MessageEdited),

            // MessageDeleted
            tl::enums::Update::DeleteMessages(tl::types::UpdateDeleteMessages {
                messages, ..
            }) => Some(Self::MessageDeleted(MessageDeletion::new(messages))),
            tl::enums::Update::DeleteChannelMessages(tl::types::UpdateDeleteChannelMessages {
                messages,
                channel_id,
                ..
            }) => Some(Self::MessageDeleted(MessageDeletion::new_with_channel(
                messages, channel_id,
            ))),

            // CallbackQuery
            tl::enums::Update::BotCallbackQuery(query) => Some(Self::CallbackQuery(
                CallbackQuery::new(client, query, chats),
            )),

            // InlineQuery
            tl::enums::Update::BotInlineQuery(query) => {
                Some(Self::InlineQuery(InlineQuery::new(client, query, chats)))
            }

            // Raw
            update => Some(Self::Raw(Box::new(update))),
        }
    }
}
