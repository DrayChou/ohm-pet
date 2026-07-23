use crate::{
    agent_ipc::AgentEvent,
    channels::{ChannelCommand, ChannelConfigStore, LarkConfig, TelegramConfig},
    task_tracker::format_duration,
};
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskNotification {
    pub source: String,
    pub title: String,
    pub event: AgentEvent,
    pub elapsed: Duration,
}

impl TaskNotification {
    fn status(&self) -> &'static str {
        if self.event == AgentEvent::Failed {
            "Task failed"
        } else {
            "Task completed"
        }
    }

    fn icon(&self) -> &'static str {
        if self.event == AgentEvent::Failed {
            "❌"
        } else {
            "✅"
        }
    }

    fn body(&self) -> String {
        format!(
            "{}\nDuration: {}",
            self.title,
            format_duration(self.elapsed)
        )
    }

    fn channel_text(&self) -> String {
        format!(
            "{} OHM Pet · {}\n{}\nDuration: {}",
            self.icon(),
            display_source(&self.source),
            self.title,
            format_duration(self.elapsed)
        )
    }
}

trait NotificationChannel {
    fn send(&self, agent: &ureq::Agent, notification: &TaskNotification) -> Result<(), String>;
}

#[derive(Debug, Clone, Copy)]
pub enum ChannelKind {
    Telegram,
    Lark,
}

impl ChannelKind {
    fn display_name(self) -> &'static str {
        match self {
            Self::Telegram => "Telegram",
            Self::Lark => "Feishu / Lark",
        }
    }
}

pub fn test_channel(kind: ChannelKind) {
    let config = ChannelConfigStore::system().and_then(|store| store.load_checked().ok());
    let _ = std::thread::Builder::new()
        .name("ohm-pet-channel-test".into())
        .spawn(move || {
            let result = match (kind, config) {
                (_, None) => Err("Channel configuration could not be loaded".to_owned()),
                (_, Some(config)) if config.http_agent().is_err() => {
                    Err("The configured proxy URL is invalid".to_owned())
                }
                (ChannelKind::Telegram, Some(config)) if !config.telegram.ready() => {
                    Err("Telegram is disabled or incomplete".to_owned())
                }
                (ChannelKind::Telegram, Some(config)) => config.telegram.send_text(
                    &config.http_agent().expect("validated proxy URL"),
                    &config.telegram.chat_id,
                    "✅ OHM Pet Telegram test message\nThe notification channel is configured correctly.",
                    None,
                ),
                (ChannelKind::Lark, Some(config)) if !config.lark.ready() => {
                    Err("Feishu / Lark is disabled or incomplete".to_owned())
                }
                (ChannelKind::Lark, Some(config)) => config.lark.send_text(
                    &config.http_agent().expect("validated proxy URL"),
                    &config.lark.receive_id_type,
                    &config.lark.receive_id,
                    "✅ OHM Pet Feishu / Lark test message\nThe notification channel is configured correctly.",
                ),
            };
            match result {
                Ok(()) => show_channel_notice(
                    &format!("{} test succeeded", kind.display_name()),
                    "A test message was sent successfully.",
                ),
                Err(error) => show_channel_notice(
                    &format!("{} test failed", kind.display_name()),
                    &error,
                ),
            }
        });
}

pub fn show_channel_notice(summary: &str, body: &str) {
    let _ = notify_rust::Notification::new()
        .appname("OHM Pet")
        .summary(summary)
        .body(body)
        .show();
}

pub fn send_channel_reply(command: &ChannelCommand, text: String) {
    let command = command.clone();
    let text = truncate_message(&text, 3_500);
    let config = ChannelConfigStore::system().map(|store| store.load());
    let _ = std::thread::Builder::new()
        .name("ohm-pet-channel-reply".into())
        .spawn(move || {
            let Some(config) = config else {
                return;
            };
            let Ok(agent) = config.http_agent() else {
                return;
            };
            if command.channel == "telegram"
                && config.telegram.ready()
                && command.conversation_id == config.telegram.chat_id.trim()
                && command
                    .sender_id
                    .parse::<i64>()
                    .is_ok_and(|id| config.telegram.allowed_user_ids.contains(&id))
            {
                let _ = config.telegram.send_text(
                    &agent,
                    &command.conversation_id,
                    &text,
                    command.reply_to_message_id,
                );
            } else if command.channel == "lark"
                && config.lark.ready()
                && config
                    .lark
                    .allowed_open_ids
                    .iter()
                    .any(|id| id == &command.sender_id)
            {
                let _ = config
                    .lark
                    .send_text(&agent, "chat_id", &command.conversation_id, &text);
            }
        });
}

pub fn notify_task(notification: TaskNotification) {
    let config = ChannelConfigStore::system().map(|store| store.load());
    let _ = std::thread::Builder::new()
        .name("ohm-pet-notification".into())
        .spawn(move || {
            send_system_notification(&notification);
            let Some(config) = config else {
                return;
            };
            let Ok(agent) = config.http_agent() else {
                eprintln!("Channel notification failed: invalid proxy URL");
                return;
            };
            if config.telegram.ready() {
                if let Err(error) = config.telegram.send(&agent, &notification) {
                    eprintln!("Telegram notification failed: {error}");
                }
            }
            if config.lark.ready() {
                if let Err(error) = config.lark.send(&agent, &notification) {
                    eprintln!("Lark notification failed: {error}");
                }
            }
        });
}

fn send_system_notification(notification: &TaskNotification) {
    let _ = notify_rust::Notification::new()
        .appname("OHM Pet")
        .summary(&format!(
            "OHM Pet · {} · {}",
            display_source(&notification.source),
            notification.status()
        ))
        .body(&notification.body())
        .show();
}

impl TelegramConfig {
    fn send_text(
        &self,
        agent: &ureq::Agent,
        chat_id: &str,
        text: &str,
        reply_to_message_id: Option<i64>,
    ) -> Result<(), String> {
        let url = format!("https://api.telegram.org/bot{}/sendMessage", self.bot_token);
        let mut body = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "disable_web_page_preview": true
        });
        if let Some(message_id) = reply_to_message_id {
            body["reply_parameters"] = serde_json::json!({ "message_id": message_id });
        }
        let _ = send_json(agent.post(&url), body, "Telegram API request failed")?;
        Ok(())
    }
}

impl NotificationChannel for TelegramConfig {
    fn send(&self, agent: &ureq::Agent, notification: &TaskNotification) -> Result<(), String> {
        self.send_text(agent, &self.chat_id, &notification.channel_text(), None)
    }
}

#[derive(Deserialize)]
struct LarkApiResponse {
    code: i64,
    #[serde(default)]
    msg: String,
}

#[derive(Deserialize)]
struct LarkTokenResponse {
    code: i64,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    tenant_access_token: String,
}

impl LarkConfig {
    fn send_text(
        &self,
        agent: &ureq::Agent,
        receive_id_type: &str,
        receive_id: &str,
        text: &str,
    ) -> Result<(), String> {
        let token_response = agent
            .post("https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal")
            .send_json(serde_json::json!({
                "app_id": self.app_id,
                "app_secret": self.app_secret
            }))
            .map_err(|_| "Lark authentication request failed".to_owned())?;
        let token: LarkTokenResponse = token_response
            .into_body()
            .read_json()
            .map_err(|_| "Lark authentication response was invalid".to_owned())?;
        if token.code != 0 || token.tenant_access_token.is_empty() {
            return Err(if token.msg.is_empty() {
                format!("Lark authentication failed with code {}", token.code)
            } else {
                format!("Lark authentication failed: {}", token.msg)
            });
        }

        let url = format!(
            "https://open.feishu.cn/open-apis/im/v1/messages?receive_id_type={}",
            receive_id_type
        );
        let content = serde_json::to_string(&serde_json::json!({
            "text": text
        }))
        .map_err(|_| "Lark message encoding failed".to_owned())?;
        let response = send_json(
            agent.post(&url).header(
                "Authorization",
                &format!("Bearer {}", token.tenant_access_token),
            ),
            serde_json::json!({
                "receive_id": receive_id,
                "msg_type": "text",
                "content": content
            }),
            "Lark message request failed",
        )?;
        let result: LarkApiResponse = response
            .into_body()
            .read_json()
            .map_err(|_| "Lark message response was invalid".to_owned())?;
        if result.code == 0 {
            Ok(())
        } else if result.msg.is_empty() {
            Err(format!("Lark message failed with code {}", result.code))
        } else {
            Err(format!("Lark message failed: {}", result.msg))
        }
    }
}

impl NotificationChannel for LarkConfig {
    fn send(&self, agent: &ureq::Agent, notification: &TaskNotification) -> Result<(), String> {
        self.send_text(
            agent,
            &self.receive_id_type,
            &self.receive_id,
            &notification.channel_text(),
        )
    }
}

fn send_json(
    request: ureq::RequestBuilder<ureq::typestate::WithBody>,
    body: serde_json::Value,
    public_error: &str,
) -> Result<ureq::http::Response<ureq::Body>, String> {
    let response = request
        .send_json(body)
        .map_err(|_| public_error.to_owned())?;
    if response.status().is_success() {
        Ok(response)
    } else {
        Err(format!("{public_error}: HTTP {}", response.status()))
    }
}

fn truncate_message(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    let mut result: String = value.chars().take(max_chars.saturating_sub(1)).collect();
    result.push('…');
    result
}

fn display_source(source: &str) -> &str {
    match source {
        "pi" => "Pi",
        "claude" => "Claude Code",
        "codex" => "Codex",
        value => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_plain_channel_completion_without_parse_mode() {
        let notification = TaskNotification {
            source: "pi".into(),
            title: "Run tests [workspace]".into(),
            event: AgentEvent::Completed,
            elapsed: Duration::from_secs(65),
        };
        assert_eq!(
            notification.channel_text(),
            "✅ OHM Pet · Pi\nRun tests [workspace]\nDuration: 01:05"
        );
    }

    #[test]
    fn truncates_remote_responses_on_character_boundaries() {
        assert_eq!(truncate_message("你好世界", 3), "你好…");
    }

    #[test]
    fn formats_failure_distinctly() {
        let notification = TaskNotification {
            source: "claude".into(),
            title: "Deploy".into(),
            event: AgentEvent::Failed,
            elapsed: Duration::from_secs(9),
        };
        assert!(notification.channel_text().starts_with("❌"));
        assert_eq!(notification.status(), "Task failed");
    }
}
