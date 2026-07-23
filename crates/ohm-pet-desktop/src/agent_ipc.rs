use crate::channels::ChannelCommand;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::{net::UdpSocket, str::FromStr, thread, time::Duration};
use winit::event_loop::EventLoopProxy;

pub const AGENT_SIGNAL_ADDRESS: &str = "127.0.0.1:47832";
const MAX_SIGNAL_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEvent {
    Working,
    Waiting,
    Completed,
    Failed,
    Idle,
}

impl FromStr for AgentEvent {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "working" | "running" | "started" => Ok(Self::Working),
            "waiting" | "needs_input" | "permission" => Ok(Self::Waiting),
            "completed" | "complete" | "done" | "success" => Ok(Self::Completed),
            "failed" | "failure" | "error" => Ok(Self::Failed),
            "idle" | "stopped" => Ok(Self::Idle),
            _ => Err(anyhow!("unsupported agent event: {value}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSignal {
    pub source: String,
    pub event: AgentEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "sessionId")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "taskId")]
    pub task_id: Option<String>,
}

impl AgentSignal {
    pub fn task_key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.source,
            self.session_id.as_deref().unwrap_or("default"),
            self.task_id.as_deref().unwrap_or("current")
        )
    }
}

#[derive(Debug, Clone)]
pub enum UserEvent {
    AgentSignal(AgentSignal),
    ChannelCommand(ChannelCommand),
}

pub fn send_signal(signal: &AgentSignal) -> Result<()> {
    let payload = serde_json::to_vec(signal).context("serialize agent signal")?;
    let socket = UdpSocket::bind("127.0.0.1:0").context("bind agent signal sender")?;
    socket
        .send_to(&payload, AGENT_SIGNAL_ADDRESS)
        .context("send agent signal")?;
    Ok(())
}

pub fn spawn_signal_listener(proxy: EventLoopProxy<UserEvent>) -> Result<()> {
    let socket = UdpSocket::bind(AGENT_SIGNAL_ADDRESS)
        .with_context(|| format!("listen for agent signals on {AGENT_SIGNAL_ADDRESS}"))?;
    socket
        .set_read_timeout(Some(Duration::from_secs(1)))
        .context("configure agent signal listener")?;
    thread::Builder::new()
        .name("ohm-pet-agent-ipc".into())
        .spawn(move || {
            let mut buffer = [0_u8; MAX_SIGNAL_BYTES];
            loop {
                match socket.recv_from(&mut buffer) {
                    Ok((size, _)) => {
                        if let Ok(signal) = serde_json::from_slice::<AgentSignal>(&buffer[..size]) {
                            if proxy.send_event(UserEvent::AgentSignal(signal)).is_err() {
                                break;
                            }
                        }
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(_) => break,
                }
            }
        })
        .context("spawn agent signal listener")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_event_aliases() {
        assert_eq!(
            "running".parse::<AgentEvent>().unwrap(),
            AgentEvent::Working
        );
        assert_eq!(
            "needs_input".parse::<AgentEvent>().unwrap(),
            AgentEvent::Waiting
        );
        assert_eq!("done".parse::<AgentEvent>().unwrap(), AgentEvent::Completed);
        assert_eq!("error".parse::<AgentEvent>().unwrap(), AgentEvent::Failed);
    }

    #[test]
    fn signal_round_trips_as_json() {
        let signal = AgentSignal {
            source: "pi".into(),
            event: AgentEvent::Completed,
            title: Some("Task complete".into()),
            session_id: Some("session-1".into()),
            task_id: Some("turn-1".into()),
        };
        let encoded = serde_json::to_vec(&signal).unwrap();
        assert_eq!(
            serde_json::from_slice::<AgentSignal>(&encoded).unwrap(),
            signal
        );
    }
}
