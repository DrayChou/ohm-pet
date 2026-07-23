use crate::{
    agent_ipc::UserEvent,
    channels::{ChannelCommand, ChannelConfigStore, LarkConfig},
};
use prost::Message as ProstMessage;
use serde::Deserialize;
use std::{
    collections::HashMap,
    net::TcpStream,
    thread,
    time::{Duration, Instant},
};
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};
use winit::event_loop::EventLoopProxy;

const ENDPOINT_URL: &str = "https://open.feishu.cn/callback/ws/endpoint";

#[derive(Clone, PartialEq, prost::Message)]
struct Header {
    #[prost(string, required, tag = "1")]
    key: String,
    #[prost(string, required, tag = "2")]
    value: String,
}

#[derive(Clone, PartialEq, prost::Message)]
struct Frame {
    #[prost(uint64, required, tag = "1")]
    seq_id: u64,
    #[prost(uint64, required, tag = "2")]
    log_id: u64,
    #[prost(int32, required, tag = "3")]
    service: i32,
    #[prost(int32, required, tag = "4")]
    method: i32,
    #[prost(message, repeated, tag = "5")]
    headers: Vec<Header>,
    #[prost(string, optional, tag = "6")]
    payload_encoding: Option<String>,
    #[prost(string, optional, tag = "7")]
    payload_type: Option<String>,
    #[prost(bytes, optional, tag = "8")]
    payload: Option<Vec<u8>>,
    #[prost(string, optional, tag = "9")]
    log_id_new: Option<String>,
}

impl Frame {
    fn header(&self, key: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|header| header.key == key)
            .map(|header| header.value.as_str())
    }

    fn set_header(&mut self, key: &str, value: String) {
        if let Some(header) = self.headers.iter_mut().find(|header| header.key == key) {
            header.value = value;
        } else {
            self.headers.push(Header {
                key: key.into(),
                value,
            });
        }
    }
}

#[derive(Deserialize)]
struct EndpointResponse {
    code: i64,
    data: Option<EndpointData>,
}

#[derive(Deserialize)]
struct EndpointData {
    #[serde(rename = "URL")]
    url: String,
    #[serde(rename = "ClientConfig")]
    client_config: Option<LarkClientConfig>,
}

#[derive(Debug, Clone, Deserialize)]
struct LarkClientConfig {
    #[serde(rename = "PingInterval", default = "default_ping_interval")]
    ping_interval: u64,
}

fn default_ping_interval() -> u64 {
    120
}

#[derive(Deserialize)]
struct EventEnvelope {
    header: EventHeader,
    event: MessageEvent,
}

#[derive(Deserialize)]
struct EventHeader {
    event_type: String,
}

#[derive(Deserialize)]
struct MessageEvent {
    sender: EventSender,
    message: EventMessage,
}

#[derive(Deserialize)]
struct EventSender {
    sender_id: EventSenderId,
}

#[derive(Deserialize)]
struct EventSenderId {
    open_id: Option<String>,
}

#[derive(Deserialize)]
struct EventMessage {
    message_id: String,
    chat_id: String,
    message_type: String,
    content: String,
}

#[derive(Deserialize)]
struct TextContent {
    text: String,
}

pub fn spawn_lark_runtime(proxy: EventLoopProxy<UserEvent>) {
    let _ = thread::Builder::new()
        .name("ohm-pet-lark-runtime".into())
        .spawn(move || run(proxy));
}

fn run(proxy: EventLoopProxy<UserEvent>) {
    let Some(store) = ChannelConfigStore::system() else {
        return;
    };
    loop {
        let config = store.load().lark;
        if !config.ready() || config.allowed_open_ids.is_empty() {
            thread::sleep(Duration::from_secs(3));
            continue;
        }
        if run_connection(&store, &config, &proxy).is_err() {
            thread::sleep(Duration::from_secs(5));
        }
    }
}

fn run_connection(
    store: &ChannelConfigStore,
    config: &LarkConfig,
    proxy: &EventLoopProxy<UserEvent>,
) -> Result<(), String> {
    let endpoint = get_endpoint(config)?;
    let mut ping_interval = endpoint
        .client_config
        .as_ref()
        .map_or(120, |value| value.ping_interval.max(10));
    let service_id = endpoint
        .url
        .split("service_id=")
        .nth(1)
        .and_then(|value| value.split('&').next())
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or_default();
    let (mut socket, _) = tungstenite::connect(&endpoint.url)
        .map_err(|_| "Lark WebSocket connection failed".to_owned())?;
    set_read_timeout(&mut socket, Duration::from_secs(1));

    let mut last_ping = Instant::now();
    let mut next_config_check = Instant::now() + Duration::from_secs(3);
    let mut fragments: HashMap<String, Vec<Option<Vec<u8>>>> = HashMap::new();
    loop {
        if Instant::now() >= next_config_check {
            let latest = store.load().lark;
            if !same_connection(config, &latest) {
                let _ = socket.close(None);
                return Ok(());
            }
            next_config_check = Instant::now() + Duration::from_secs(3);
        }
        if last_ping.elapsed() >= Duration::from_secs(ping_interval) {
            send_ping(&mut socket, service_id)?;
            last_ping = Instant::now();
        }
        match socket.read() {
            Ok(Message::Binary(bytes)) => {
                let mut frame = Frame::decode(bytes.as_ref())
                    .map_err(|_| "Lark WebSocket frame was invalid".to_owned())?;
                if frame.method == 0 && frame.header("type") == Some("pong") {
                    if let Some(payload) = frame.payload.as_deref() {
                        if let Ok(config) = serde_json::from_slice::<LarkClientConfig>(payload) {
                            ping_interval = config.ping_interval.max(10);
                        }
                    }
                    continue;
                }
                if frame.method != 1 || frame.header("type") != Some("event") {
                    continue;
                }
                let started = Instant::now();
                if let Some(payload) = combine_payload(&frame, &mut fragments) {
                    if let Some(command) = command_from_payload(config, &payload) {
                        if proxy
                            .send_event(UserEvent::ChannelCommand(command))
                            .is_err()
                        {
                            return Ok(());
                        }
                    }
                    acknowledge(&mut socket, &mut frame, started.elapsed())?;
                }
            }
            Ok(Message::Ping(payload)) => {
                socket
                    .send(Message::Pong(payload))
                    .map_err(|_| "Lark WebSocket pong failed".to_owned())?;
            }
            Ok(Message::Close(_)) => return Err("Lark WebSocket closed".into()),
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return Err("Lark WebSocket receive failed".into()),
        }
    }
}

fn get_endpoint(config: &LarkConfig) -> Result<EndpointData, String> {
    let response = ureq::post(ENDPOINT_URL)
        .header("locale", "zh")
        .send_json(serde_json::json!({
            "AppID": config.app_id,
            "AppSecret": config.app_secret
        }))
        .map_err(|_| "Lark WebSocket endpoint request failed".to_owned())?;
    let endpoint: EndpointResponse = response
        .into_body()
        .read_json()
        .map_err(|_| "Lark WebSocket endpoint response was invalid".to_owned())?;
    if endpoint.code != 0 {
        return Err(format!(
            "Lark WebSocket endpoint rejected with code {}",
            endpoint.code
        ));
    }
    endpoint
        .data
        .filter(|data| !data.url.is_empty())
        .ok_or_else(|| "Lark WebSocket endpoint was empty".to_owned())
}

fn send_ping(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    service_id: i32,
) -> Result<(), String> {
    let frame = Frame {
        seq_id: 0,
        log_id: 0,
        service: service_id,
        method: 0,
        headers: vec![Header {
            key: "type".into(),
            value: "ping".into(),
        }],
        payload_encoding: None,
        payload_type: None,
        payload: None,
        log_id_new: None,
    };
    socket
        .send(Message::Binary(frame.encode_to_vec().into()))
        .map_err(|_| "Lark WebSocket ping failed".to_owned())
}

fn acknowledge(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    frame: &mut Frame,
    elapsed: Duration,
) -> Result<(), String> {
    frame.set_header("biz_rt", elapsed.as_millis().to_string());
    frame.payload = Some(br#"{"code":200,"headers":null,"data":null}"#.to_vec());
    socket
        .send(Message::Binary(frame.encode_to_vec().into()))
        .map_err(|_| "Lark WebSocket acknowledgement failed".to_owned())
}

fn combine_payload(
    frame: &Frame,
    fragments: &mut HashMap<String, Vec<Option<Vec<u8>>>>,
) -> Option<Vec<u8>> {
    let payload = frame.payload.clone().unwrap_or_default();
    if payload.len() > 2 * 1024 * 1024 {
        return None;
    }
    let sum = frame
        .header("sum")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1);
    if sum <= 1 {
        return Some(payload);
    }
    if sum > 64 {
        return None;
    }
    let sequence = frame
        .header("seq")
        .and_then(|value| value.parse::<usize>().ok())?;
    let message_id = frame.header("message_id")?.to_owned();
    if sequence >= sum {
        return None;
    }
    if !fragments.contains_key(&message_id) && fragments.len() >= 64 {
        fragments.clear();
    }
    let entry = fragments
        .entry(message_id.clone())
        .or_insert_with(|| vec![None; sum]);
    if entry.len() != sum {
        return None;
    }
    entry[sequence] = Some(payload);
    if entry.iter().any(Option::is_none) {
        return None;
    }
    let total_size: usize = entry.iter().flatten().map(Vec::len).sum();
    if total_size > 2 * 1024 * 1024 {
        fragments.remove(&message_id);
        return None;
    }
    let result = entry
        .iter()
        .filter_map(|part| part.as_ref())
        .flatten()
        .copied()
        .collect();
    fragments.remove(&message_id);
    Some(result)
}

fn command_from_payload(config: &LarkConfig, payload: &[u8]) -> Option<ChannelCommand> {
    let envelope: EventEnvelope = serde_json::from_slice(payload).ok()?;
    if envelope.header.event_type != "im.message.receive_v1"
        || envelope.event.message.message_type != "text"
    {
        return None;
    }
    let open_id = envelope.event.sender.sender_id.open_id?;
    if !config
        .allowed_open_ids
        .iter()
        .any(|value| value == &open_id)
    {
        return None;
    }
    let content: TextContent = serde_json::from_str(&envelope.event.message.content).ok()?;
    let text = content.text.trim().to_owned();
    if !text.starts_with('/') {
        return None;
    }
    Some(ChannelCommand {
        channel: "lark".into(),
        conversation_id: envelope.event.message.chat_id,
        sender_id: open_id,
        text,
        reply_to_message_id: envelope.event.message.message_id.parse().ok(),
    })
}

fn same_connection(current: &LarkConfig, latest: &LarkConfig) -> bool {
    latest.ready() && !latest.allowed_open_ids.is_empty() && current == latest
}

fn set_read_timeout(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>, timeout: Duration) {
    let result = match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => stream.set_read_timeout(Some(timeout)),
        MaybeTlsStream::NativeTls(stream) => stream.get_ref().set_read_timeout(Some(timeout)),
        _ => return,
    };
    let _ = result;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> LarkConfig {
        LarkConfig {
            enabled: true,
            app_id: "app".into(),
            app_secret: "secret".into(),
            receive_id_type: "chat_id".into(),
            receive_id: "chat".into(),
            allowed_open_ids: vec!["ou_allowed".into()],
        }
    }

    #[test]
    fn parses_allowlisted_lark_text_command() {
        let payload = br#"{
          "header":{"event_type":"im.message.receive_v1"},
          "event":{
            "sender":{"sender_id":{"open_id":"ou_allowed"}},
            "message":{"message_id":"12","chat_id":"oc_chat","message_type":"text","content":"{\"text\":\"/tasks\"}"}
          }
        }"#;
        let command = command_from_payload(&config(), payload).unwrap();
        assert_eq!(command.channel, "lark");
        assert_eq!(command.text, "/tasks");
        assert_eq!(command.conversation_id, "oc_chat");
    }

    #[test]
    fn rejects_unknown_lark_sender() {
        let payload = br#"{
          "header":{"event_type":"im.message.receive_v1"},
          "event":{
            "sender":{"sender_id":{"open_id":"ou_unknown"}},
            "message":{"message_id":"12","chat_id":"oc_chat","message_type":"text","content":"{\"text\":\"/tasks\"}"}
          }
        }"#;
        assert!(command_from_payload(&config(), payload).is_none());
    }
}
