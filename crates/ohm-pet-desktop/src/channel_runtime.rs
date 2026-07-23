use crate::{
    agent_ipc::UserEvent,
    channels::{ChannelCommand, ChannelConfig, ChannelConfigStore, TelegramConfig},
};
use serde::{Deserialize, Serialize};
use std::{fs, thread, time::Duration};
use winit::event_loop::EventLoopProxy;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ChannelRuntimeState {
    telegram_update_offset: i64,
    telegram_bot_id: String,
}

#[derive(Debug, Deserialize)]
struct TelegramUpdates {
    ok: bool,
    #[serde(default)]
    result: Vec<TelegramUpdate>,
}

#[derive(Debug, Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    message: Option<TelegramMessage>,
}

#[derive(Debug, Deserialize)]
struct TelegramMessage {
    message_id: i64,
    text: Option<String>,
    chat: TelegramChat,
    from: Option<TelegramUser>,
}

#[derive(Debug, Deserialize)]
struct TelegramChat {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct TelegramUser {
    id: i64,
}

pub fn spawn_channel_runtime(proxy: EventLoopProxy<UserEvent>) {
    let _ = thread::Builder::new()
        .name("ohm-pet-channel-runtime".into())
        .spawn(move || run(proxy));
}

fn run(proxy: EventLoopProxy<UserEvent>) {
    let Some(store) = ChannelConfigStore::system() else {
        return;
    };
    let mut state = load_state(&store);
    let mut telegram_initialized = store.state_path().exists();
    loop {
        let config = store.load();
        if !config.telegram.ready() || config.telegram.allowed_user_ids.is_empty() {
            thread::sleep(Duration::from_secs(3));
            continue;
        }
        let bot_id = config
            .telegram
            .bot_token
            .split_once(':')
            .map_or("unknown", |(id, _)| id);
        if state.telegram_bot_id != bot_id {
            state.telegram_update_offset = 0;
            state.telegram_bot_id = bot_id.to_owned();
            telegram_initialized = false;
        }
        if !telegram_initialized {
            if let Ok(updates) = poll_telegram(&config, -1, 0) {
                for update in updates {
                    state.telegram_update_offset = state
                        .telegram_update_offset
                        .max(update.update_id.saturating_add(1));
                }
                let _ = save_state(&store, &state);
                telegram_initialized = true;
            } else {
                thread::sleep(Duration::from_secs(5));
            }
            continue;
        }
        match poll_telegram(&config, state.telegram_update_offset, 20) {
            Ok(updates) => {
                let previous_offset = state.telegram_update_offset;
                for update in updates {
                    state.telegram_update_offset = state
                        .telegram_update_offset
                        .max(update.update_id.saturating_add(1));
                    let Some(command) = command_from_update(&config.telegram, update) else {
                        continue;
                    };
                    if proxy
                        .send_event(UserEvent::ChannelCommand(command))
                        .is_err()
                    {
                        return;
                    }
                }
                if state.telegram_update_offset != previous_offset {
                    let _ = save_state(&store, &state);
                }
            }
            Err(_) => thread::sleep(Duration::from_secs(5)),
        }
    }
}

fn poll_telegram(
    config: &ChannelConfig,
    offset: i64,
    timeout_seconds: u64,
) -> Result<Vec<TelegramUpdate>, String> {
    let url = format!(
        "https://api.telegram.org/bot{}/getUpdates?offset={offset}&timeout={timeout_seconds}&allowed_updates=%5B%22message%22%5D",
        config.telegram.bot_token
    );
    let response = config
        .http_agent()?
        .get(&url)
        .call()
        .map_err(|_| "Telegram polling failed".to_owned())?;
    let updates: TelegramUpdates = response
        .into_body()
        .read_json()
        .map_err(|_| "Telegram polling response was invalid".to_owned())?;
    if updates.ok {
        Ok(updates.result)
    } else {
        Err("Telegram polling was rejected".into())
    }
}

fn command_from_update(config: &TelegramConfig, update: TelegramUpdate) -> Option<ChannelCommand> {
    let message = update.message?;
    let sender_id = message.from?.id;
    if !config.allowed_user_ids.contains(&sender_id) {
        return None;
    }
    if message.chat.id.to_string() != config.chat_id.trim() {
        return None;
    }
    let text = message.text?.trim().to_owned();
    if text.is_empty() || !text.starts_with('/') {
        return None;
    }
    Some(ChannelCommand {
        channel: "telegram".into(),
        conversation_id: message.chat.id.to_string(),
        sender_id: sender_id.to_string(),
        text,
        reply_to_message_id: Some(message.message_id),
    })
}

fn load_state(store: &ChannelConfigStore) -> ChannelRuntimeState {
    fs::read_to_string(store.state_path())
        .ok()
        .and_then(|source| serde_json::from_str(&source).ok())
        .unwrap_or_default()
}

fn save_state(store: &ChannelConfigStore, state: &ChannelRuntimeState) -> std::io::Result<()> {
    let source = serde_json::to_vec(state).map_err(std::io::Error::other)?;
    store.save_runtime_state(&source)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> TelegramConfig {
        TelegramConfig {
            enabled: true,
            bot_token: "token".into(),
            chat_id: "42".into(),
            allowed_user_ids: vec![7],
        }
    }

    #[test]
    fn accepts_only_allowlisted_commands_from_configured_chat() {
        let update = TelegramUpdate {
            update_id: 1,
            message: Some(TelegramMessage {
                message_id: 2,
                text: Some("/tasks".into()),
                chat: TelegramChat { id: 42 },
                from: Some(TelegramUser { id: 7 }),
            }),
        };
        assert_eq!(
            command_from_update(&config(), update).unwrap().text,
            "/tasks"
        );
    }

    #[test]
    fn rejects_plain_text_and_unknown_users() {
        let update = TelegramUpdate {
            update_id: 1,
            message: Some(TelegramMessage {
                message_id: 2,
                text: Some("hello".into()),
                chat: TelegramChat { id: 42 },
                from: Some(TelegramUser { id: 99 }),
            }),
        };
        assert!(command_from_update(&config(), update).is_none());
    }
}
