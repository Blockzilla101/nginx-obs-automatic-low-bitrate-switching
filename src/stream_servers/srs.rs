use std::string::String;

use async_trait::async_trait;
use log::{error, trace};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{default_reqwest_client, Bsl, StreamServersCommands, SwitchLogic};
use crate::switcher::{SwitchType, Triggers};

#[derive(Deserialize, Debug)]
pub struct StatKbps {
    recv_30s: u64,
}

#[derive(Deserialize, Debug)]
pub struct Stat {
    pub id: String,
    pub name: String,
    pub send_bytes: u64,
    pub recv_bytes: f64,
    pub live_ms: u64,
    pub kbps: StatKbps,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SRS {
    /// URL to SRS stats page (ex; http://127.0.0.1:1985/api/v1/streams )
    pub stats_url: String,

    /// StreamID of the where you are publishing the feed. (ex; publish/live/<feed1> )
    pub publisher: String,

    /// Client to make HTTP requests with
    #[serde(skip, default = "default_reqwest_client")]
    pub client: reqwest::Client,
}

impl SRS {
    pub async fn get_stats(&self) -> Option<Stat> {
        let res = match self.client.get(&self.stats_url).send().await {
            Ok(res) => res,
            Err(_) => {
                error!("Stats page ({}) is unreachable", self.stats_url);
                return None;
            }
        };

        if res.status() != reqwest::StatusCode::OK {
            error!("Error accessing stats page ({})", self.stats_url);
            return None;
        }

        let text = match res.text().await {
            Ok(text) => text,
            Err(_) => {
                error!("Error reading stats page ({})", self.stats_url);
                return None;
            }
        };

        let data: Value = serde_json::from_str(&text).ok()?;
        let streams: Vec<Value> = match data["streams"].as_array() {
            Some(streams) => streams.to_owned(),
            None => {
                trace!("Error parsing stats ({})", self.stats_url);
                return None;
            }
        };

        let publisher = match streams.into_iter().find(|x| x["name"] == self.publisher) {
            Some(publisher) => publisher.to_owned(),
            None => {
                trace!(
                    "Publisher ({}) not found in stats ({})",
                    self.publisher,
                    self.stats_url
                );
                return None;
            }
        };

        let stream: Stat = match serde_json::from_value(publisher.to_owned()) {
            Ok(stats) => stats,
            Err(error) => {
                trace!("{}", &data);
                error!("Error parsing stats ({}) {}", self.stats_url, error);
                return None;
            }
        };

        trace!("{:#?}", stream);
        Some(stream)
    }
}

#[async_trait]
#[typetag::serde]
impl SwitchLogic for SRS {
    async fn switch(&self, triggers: &Triggers) -> SwitchType {
        let stats = match self.get_stats().await {
            Some(b) => b,
            None => return SwitchType::Offline,
        };

        if let Some(offline) = triggers.offline {
            if stats.kbps.recv_30s > 0 && stats.kbps.recv_30s <= offline.into() {
                return SwitchType::Offline;
            }
        }

        if stats.kbps.recv_30s == 0 {
            return SwitchType::Previous;
        }

        if let Some(low) = triggers.low {
            if stats.kbps.recv_30s <= low.into() {
                return SwitchType::Low;
            }
        }

        return SwitchType::Normal;
    }
}

#[async_trait]
#[typetag::serde]
impl StreamServersCommands for SRS {
    async fn bitrate(&self) -> super::Bitrate {
        let stats = match self.get_stats().await {
            Some(stats) => stats,
            None => return super::Bitrate { message: None },
        };

        let message = format!("{} Kbps", stats.kbps.recv_30s);
        super::Bitrate {
            message: Some(message),
        }
    }

    async fn source_info(&self) -> Option<String> {
        let stats = self.get_stats().await?;

        let bitrate = format!("{} Kbps", stats.kbps.recv_30s);

        Some(format!("{}", bitrate))
    }
}

#[typetag::serde]
impl Bsl for SRS {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
