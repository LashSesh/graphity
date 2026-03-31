// isls-gateway/src/ws.rs — WebSocket event hub for real-time Studio updates

use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use serde::{Deserialize, Serialize};

/// Event types flowing through the WebSocket
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    Tick,
    Crystal,
    Gate,
    Alert,
    Metric,
    ForgeProgress,
    FoundryProgress,
    Heartbeat,
}

/// A WebSocket event message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsEvent {
    #[serde(rename = "type")]
    pub event_type: EventType,
    pub data: serde_json::Value,
    pub timestamp: f64,
}

impl WsEvent {
    pub fn new(event_type: EventType, data: serde_json::Value) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        Self { event_type, data, timestamp: now }
    }

    pub fn heartbeat() -> Self {
        Self::new(EventType::Heartbeat, serde_json::json!({"status": "ok"}))
    }

    pub fn forge_progress(phase: &str, index: usize, total: usize, atom_name: &str, source: &str, status: &str) -> Self {
        Self::new(EventType::ForgeProgress, serde_json::json!({
            "phase": phase,
            "index": index,
            "total": total,
            "atom_name": atom_name,
            "source": source,
            "status": status,
        }))
    }

    pub fn foundry_progress(phase: &str, attempt: usize, max: usize, status: &str, error: Option<&str>) -> Self {
        Self::new(EventType::FoundryProgress, serde_json::json!({
            "phase": phase,
            "attempt": attempt,
            "max": max,
            "status": status,
            "error": error,
        }))
    }
}

/// Subscription request from a client
#[derive(Debug, Deserialize)]
pub struct SubscribeRequest {
    pub subscribe: Vec<EventType>,
}

/// Shared event hub — broadcasts events to all connected clients
#[derive(Clone)]
pub struct EventHub {
    sender: broadcast::Sender<WsEvent>,
    /// Track connected client count
    pub client_count: Arc<RwLock<usize>>,
}

impl EventHub {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            client_count: Arc::new(RwLock::new(0)),
        }
    }

    pub fn publish(&self, event: WsEvent) {
        // Ignore send errors (no subscribers)
        let _ = self.sender.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<WsEvent> {
        self.sender.subscribe()
    }
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new(256)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ws_event_serialization() {
        let event = WsEvent::heartbeat();
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"heartbeat\""));
        assert!(json.contains("\"status\":\"ok\""));
    }

    #[test]
    fn test_forge_progress_event() {
        let event = WsEvent::forge_progress("atom", 3, 9, "router", "oracle", "generating");
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"phase\":\"atom\""));
        assert!(json.contains("\"index\":3"));
        assert!(json.contains("forge_progress"));
    }

    #[test]
    fn test_foundry_progress_event() {
        let event = WsEvent::foundry_progress("compile", 2, 5, "fail", Some("error[E0277]"));
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("foundry_progress"));
        assert!(json.contains("\"attempt\":2"));
    }

    #[test]
    fn test_event_hub_publish_subscribe() {
        let hub = EventHub::new(16);
        let mut rx = hub.subscribe();
        hub.publish(WsEvent::heartbeat());
        let event = rx.try_recv().unwrap();
        assert_eq!(event.event_type, EventType::Heartbeat);
    }

    #[test]
    fn test_subscribe_request_deserialization() {
        let json = r#"{"subscribe":["tick","crystal","forge_progress"]}"#;
        let req: SubscribeRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.subscribe.len(), 3);
        assert!(req.subscribe.contains(&EventType::Tick));
        assert!(req.subscribe.contains(&EventType::ForgeProgress));
    }
}
