use crate::{
    config::{self, Liveu as Config_liveu},
    error::Error,
};
use read_input::prelude::*;
use reqwest::{
    header::{ACCEPT, ACCEPT_LANGUAGE, AUTHORIZATION, CONTENT_TYPE},
    Method, StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use uuid::Uuid;

const APPLICATION_ID: &str = "SlZ3SHqiqtYJRkF0zO";
const LIVEU_API: &str = "https://lu-central.liveu.tv/luc/luc-core-web/rest/v0";
const LIVEU_API_V2: &str = "https://lu-central.liveu.tv/luc/luc-core-web/rest/v2";
const SOLO_API: &str = "https://solo-api.liveu.tv/v1_prod";

#[derive(Deserialize)]
struct Res {
    data: Data,
}

#[derive(Deserialize)]
struct Data {
    response: AuthRes,
}

#[derive(Deserialize, Debug, Clone)]
struct AuthRes {
    access_token: String,
}

#[derive(Debug)]
struct AuthCredentials {
    access_token: String,
    user_session: String,
}

#[derive(Deserialize, Debug)]
pub struct UnitInterfaces {
    pub interfaces: Vec<Interface>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Interface {
    pub connected: bool,
    pub name: String,
    pub downlink_kbps: u32,
    pub uplink_kbps: u32,
    pub enabled: bool,
    pub port: String,
    pub technology: String,
    pub up_signal_quality: u32,
    pub down_signal_quality: u32,
    pub active_sim: Option<String>,
    pub is_currently_roaming: bool,
    pub kbps: u32,
    pub signal_quality: u32,
}

#[derive(Deserialize, Debug)]
pub struct Unit {
    pub id: String,
    pub reg_code: String,
    pub status: String,
    pub name: String,
    #[serde(default)]
    pub sw_version: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct Inventories {
    pub units: Vec<Unit>,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    pub connected: bool,
    pub percentage: u8,
    pub run_time_to_empty: u32,
    pub discharging: bool,
    pub charging: bool,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Video {
    pub resolution: Option<String>,
    pub bitrate: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DelayReq {
    pub unit: Delay,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Delay {
    pub delay: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    pub id: String,
    pub name: String,
    pub provider: Option<String>,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareInfo {
    pub current: Option<String>,
    pub available: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamToolsSummary {
    pub enabled: bool,
    pub active: bool,
    pub zone: Option<String>,
    pub profile: Option<String>,
    pub fps: Option<String>,
    pub video_bitrate: Option<String>,
    pub audio_bitrate: Option<String>,
    pub destinations: Vec<String>,
    pub logo_enabled: bool,
    pub fallback_slate_ready: bool,
}

impl StreamToolsSummary {
    fn disabled() -> Self {
        Self {
            enabled: false,
            active: false,
            zone: None,
            profile: None,
            fps: None,
            video_bitrate: None,
            audio_bitrate: None,
            destinations: Vec::new(),
            logo_enabled: false,
            fallback_slate_ready: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeUnitConfig {
    pub name: Option<String>,
    pub model: Option<String>,
    pub status: Option<String>,
    pub destination: Option<String>,
    pub resolution: Option<String>,
    pub bitrate: Option<u32>,
    pub delay: Option<u64>,
    pub audio_channels: Option<String>,
    pub firmware: Option<String>,
}

#[derive(Debug, Clone)]
struct InventoryMetadata {
    db_id: String,
    erp_id: String,
}

#[derive(Debug, Clone, Default)]
struct UnitDetails {
    selected_destination: Option<String>,
    selected_channel: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct UnitSummary {
    name: Option<String>,
    model: Option<String>,
    status: Option<String>,
    sw_version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Liveu {
    access_token: Arc<Mutex<String>>,
    user_session: Arc<Mutex<String>>,
    config: Config_liveu,
}

impl Liveu {
    pub async fn authenticate(config: Config_liveu) -> Result<Self, Error> {
        let credentials = if let Ok(credentials) = Self::get_access_token(&config).await {
            credentials
        } else {
            return Err(Error::InvalidCredentials);
        };

        Ok(Liveu {
            access_token: Arc::new(Mutex::new(credentials.access_token)),
            user_session: Arc::new(Mutex::new(credentials.user_session)),
            config,
        })
    }

    async fn get_access_token(config: &Config_liveu) -> Result<AuthCredentials, Error> {
        let user_session = Uuid::new_v4();
        let client = reqwest::Client::new();

        let res = client
            .post("https://solo-api.liveu.tv/v1_prod/zendesk/userlogin")
            .basic_auth(&config.email, Some(&config.password))
            .header(ACCEPT, "application/json, text/plain, */*")
            .header(ACCEPT_LANGUAGE, "en-US,en;q=0.9")
            .header(CONTENT_TYPE, "application/json;charset=UTF-8")
            .header(
                "x-user-name",
                format!("{}{}", &config.email, &user_session.to_string()),
            )
            .body(r#"{"return_to":"https://solo.liveu.tv/#/dashboard/units"}"#)
            .send()
            .await?
            .json::<Res>()
            .await?;

        Ok(AuthCredentials {
            access_token: res.data.response.access_token,
            user_session: user_session.to_string(),
        })
    }

    async fn refresh_access_token(&self) -> Result<(), Error> {
        let credentials = Self::get_access_token(&self.config).await?;
        *self.access_token.lock().await = credentials.access_token;
        *self.user_session.lock().await = credentials.user_session;
        Ok(())
    }

    /// Sends the specified request. Gets a new token if unauthorized.
    pub async fn send_request<T: Serialize + Clone>(
        &self,
        method: Method,
        url: &str,
        payload: Option<T>,
    ) -> Result<reqwest::Response, Error> {
        let mut res = self
            .try_send_request(method.clone(), url, payload.clone())
            .await?;

        if res.status() == 401 {
            self.refresh_access_token().await?;

            res = self
                .try_send_request(method.clone(), url, payload.clone())
                .await?;
        }

        Ok(res)
    }

    pub async fn try_send_request<T: Serialize>(
        &self,
        method: Method,
        url: &str,
        payload: Option<T>,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let client = reqwest::Client::new();

        let mut client = client
            .request(method, url)
            .header(ACCEPT, "application/json, text/plain, */*")
            .header(ACCEPT_LANGUAGE, "en-US,en;q=0.9")
            .header(
                AUTHORIZATION,
                format!("Bearer {}", { &self.access_token.lock().await }),
            )
            .header("application-id", APPLICATION_ID);

        if let Some(data) = payload {
            client = client.json(&data);
        }

        client.send().await
    }

    async fn send_solo_request(&self, url: &str) -> Result<reqwest::Response, Error> {
        let mut res = self.try_send_solo_request(url).await?;

        if res.status() == StatusCode::UNAUTHORIZED {
            self.refresh_access_token().await?;
            res = self.try_send_solo_request(url).await?;
        }

        Ok(res)
    }

    async fn try_send_solo_request(&self, url: &str) -> Result<reqwest::Response, reqwest::Error> {
        reqwest::Client::new()
            .get(url)
            .header(ACCEPT, "application/json, text/plain, */*")
            .header(ACCEPT_LANGUAGE, "en-US,en;q=0.9")
            .header(
                AUTHORIZATION,
                format!("Bearer {}", { &self.access_token.lock().await }),
            )
            .header(
                "x-user-uuid",
                { &self.user_session.lock().await }.to_string(),
            )
            .send()
            .await
    }

    pub async fn get_inventories(&self) -> Result<Inventories, Error> {
        let res = self
            .send_request(
                Method::GET,
                &format!("{}/inventories", LIVEU_API),
                None::<()>,
            )
            .await?;

        if res.status().is_client_error() {
            return Err(Error::NoInventoriesFound);
        }

        let res_json: Value = res.json().await?;
        Ok(serde_json::from_value::<Inventories>(
            res_json["data"]["inventories"][0].to_owned(),
        )?)
    }

    async fn get_inventory_metadata(&self) -> Result<InventoryMetadata, Error> {
        let res = self
            .send_request(
                Method::GET,
                &format!("{}/inventories/?limit=1000", LIVEU_API_V2),
                None::<()>,
            )
            .await?;

        if !res.status().is_success() {
            return Err(Error::NoInventoriesFound);
        }

        let value: Value = res.json().await?;
        let inventories = value
            .as_array()
            .or_else(|| value.get("data").and_then(Value::as_array))
            .ok_or(Error::NoInventoriesFound)?;
        let inventory = inventories.first().ok_or(Error::NoInventoriesFound)?;

        Ok(InventoryMetadata {
            db_id: field_string(inventory, &["dbId", "db_id"]).ok_or(Error::NoInventoriesFound)?,
            erp_id: field_string(inventory, &["erpId", "erp_id", "id"])
                .ok_or(Error::NoInventoriesFound)?,
        })
    }

    async fn get_unit_details(&self, boss_id: &str) -> Result<UnitDetails, Error> {
        let res = self
            .send_request(
                Method::GET,
                &format!(
                    "{}/units/{}?fields=channels,services,selected_channel,selected_destination",
                    LIVEU_API, boss_id
                ),
                None::<()>,
            )
            .await?;

        if !res.status().is_success() {
            return Err(Error::StatusNotAvailable);
        }

        let value: Value = res.json().await?;
        let unit = value
            .get("data")
            .and_then(|data| data.get("unit"))
            .unwrap_or(&value);

        Ok(UnitDetails {
            selected_destination: field_string(
                unit,
                &["selected_destination", "selectedDestination"],
            ),
            selected_channel: field_string(unit, &["selected_channel", "selectedChannel"]),
        })
    }

    async fn get_unit_summary(&self, boss_id: &str) -> Result<UnitSummary, Error> {
        let inventory = self.get_inventory_metadata().await?;
        let res = self
            .send_request(
                Method::GET,
                &format!(
                    "{}/inventories/{}/units/light?soloOnly=true",
                    LIVEU_API, inventory.erp_id
                ),
                None::<()>,
            )
            .await?;

        if !res.status().is_success() {
            return Err(Error::NoUnitsFound);
        }

        let value: Value = res.json().await?;
        let units = value
            .get("data")
            .and_then(|data| data.get("units"))
            .and_then(Value::as_array)
            .or_else(|| value.get("units").and_then(Value::as_array))
            .ok_or(Error::NoUnitsFound)?;
        let unit = units
            .iter()
            .find(|unit| {
                field_string(unit, &["BOSSID", "bossId", "id"]).as_deref() == Some(boss_id)
            })
            .ok_or(Error::NoUnitsFound)?;

        Ok(UnitSummary {
            name: field_string(unit, &["name", "unitName"]),
            model: field_string(unit, &["product", "model", "deviceType", "generation"]),
            status: field_string(unit, &["status"]),
            sw_version: field_string(unit, &["sw_version", "swVersion"]),
        })
    }

    pub async fn get_destinations(&self, boss_id: &str) -> Result<Vec<Destination>, Error> {
        let (inventory, unit_details) = tokio::join!(
            self.get_inventory_metadata(),
            self.get_unit_details(boss_id)
        );
        let inventory = inventory?;
        let selected_destination = unit_details?.selected_destination;

        let res = self
            .send_request(
                Method::GET,
                &format!(
                    "{}/inventories/{}/destinations",
                    LIVEU_API_V2, inventory.db_id
                ),
                None::<()>,
            )
            .await?;

        if !res.status().is_success() {
            return Err(Error::StatusNotAvailable);
        }

        let value: Value = res.json().await?;
        let destinations = value
            .as_array()
            .or_else(|| value.get("data").and_then(Value::as_array))
            .ok_or(Error::StatusNotAvailable)?;

        Ok(destinations
            .iter()
            .filter_map(|destination| {
                let id = field_string(destination, &["id", "externalId"])?;
                let name = field_string(destination, &["name", "title"])
                    .unwrap_or_else(|| "Unnamed".to_string());
                let provider = field_string(
                    destination,
                    &["streamingProvider", "streaming_provider", "type"],
                );
                let selected = selected_destination.as_deref() == Some(id.as_str());

                Some(Destination {
                    id,
                    name,
                    provider,
                    selected,
                })
            })
            .collect())
    }

    async fn get_available_firmware(&self, boss_id: &str) -> Result<Vec<String>, Error> {
        let res = self
            .send_request(
                Method::GET,
                &format!("{}/units/{}/swVersions", LIVEU_API_V2, boss_id),
                None::<()>,
            )
            .await?;

        if !res.status().is_success() {
            return Err(Error::StatusNotAvailable);
        }

        let value: Value = res.json().await?;
        let versions = value
            .as_array()
            .or_else(|| value.get("data").and_then(Value::as_array))
            .ok_or(Error::StatusNotAvailable)?;
        let mut versions: Vec<String> = versions
            .iter()
            .filter_map(|version| field_string(version, &["versionNumber", "version"]))
            .collect();
        versions.sort();
        versions.dedup();
        Ok(versions)
    }

    pub async fn get_firmware_info(&self, boss_id: &str) -> Result<FirmwareInfo, Error> {
        let (available, unit) = tokio::join!(
            self.get_available_firmware(boss_id),
            self.get_unit_summary(boss_id)
        );

        Ok(FirmwareInfo {
            current: unit.ok().and_then(|unit| unit.sw_version),
            available: available?,
        })
    }

    async fn get_audio_channels(&self, boss_id: &str) -> Result<String, Error> {
        let res = self
            .send_request(
                Method::GET,
                &format!("{}/units/{}/configurationV2", LIVEU_API_V2, boss_id),
                None::<()>,
            )
            .await?;

        if !res.status().is_success() {
            return Err(Error::StatusNotAvailable);
        }

        let value: Value = res.json().await?;
        nested_value(
            &value,
            &[
                "singleCamera",
                "streamingInformation",
                "sdi0",
                "audio",
                "numberOfChannels",
            ],
        )
        .and_then(value_string)
        .ok_or(Error::StatusNotAvailable)
    }

    async fn get_stream_tools_destinations(&self, boss_id: &str) -> Result<Vec<String>, Error> {
        let res = self
            .send_solo_request(&format!(
                "{}/streamtooldestinations?unit_id={}",
                SOLO_API, boss_id
            ))
            .await?;

        if !res.status().is_success() {
            return Err(Error::StatusNotAvailable);
        }

        let value: Value = res.json().await?;
        let destinations = nested_value(&value, &["data", "response"])
            .and_then(Value::as_array)
            .or_else(|| value.get("response").and_then(Value::as_array))
            .map_or(&[][..], Vec::as_slice);

        Ok(destinations
            .iter()
            .filter_map(|destination| field_string(destination, &["title", "name"]))
            .collect())
    }

    pub async fn get_stream_tools(&self, boss_id: &str) -> Result<StreamToolsSummary, Error> {
        let details = self.get_unit_details(boss_id).await.unwrap_or_default();
        let requested_zone = stream_tools_zone(details.selected_channel.as_deref());
        let res = self
            .send_solo_request(&format!(
                "{}/streamtools?bossId={}&STZone={}",
                SOLO_API, boss_id, requested_zone
            ))
            .await?;

        if matches!(
            res.status(),
            StatusCode::FORBIDDEN | StatusCode::NOT_FOUND | StatusCode::NO_CONTENT
        ) {
            return Ok(StreamToolsSummary::disabled());
        }
        if !res.status().is_success() {
            return Err(Error::StatusNotAvailable);
        }

        let value: Value = res.json().await?;
        let data = nested_value(&value, &["data", "response", "data"])
            .or_else(|| nested_value(&value, &["response", "data"]));
        let Some(data) = data.filter(|data| !data.is_null()) else {
            return Ok(StreamToolsSummary::disabled());
        };
        let destinations = self
            .get_stream_tools_destinations(boss_id)
            .await
            .unwrap_or_default();

        Ok(StreamToolsSummary {
            enabled: true,
            active: field_bool(data, &["toggle_state", "toggleState"]).unwrap_or(false),
            zone: field_string(data, &["selected_zone", "selectedZone"])
                .or_else(|| Some(requested_zone.to_string())),
            profile: field_string(data, &["profile"]),
            fps: field_string(data, &["fps"]),
            video_bitrate: field_string(data, &["video_bitrate", "videoBitrate"]),
            audio_bitrate: field_string(data, &["audio_bitrate", "audioBitrate"]),
            destinations,
            logo_enabled: field_string(data, &["logo_url", "logoUrl"]).is_some(),
            fallback_slate_ready: field_string(data, &["fallback_slate_url", "fallBackSlateUrl"])
                .is_some(),
        })
    }

    pub async fn get_safe_unit_config(&self, boss_id: &str) -> Result<SafeUnitConfig, Error> {
        let (unit, destinations, video, delay, audio_channels) = tokio::join!(
            self.get_unit_summary(boss_id),
            self.get_destinations(boss_id),
            self.get_video(boss_id),
            self.get_delay(boss_id),
            self.get_audio_channels(boss_id)
        );

        if unit.is_err()
            && destinations.is_err()
            && video.is_err()
            && delay.is_err()
            && audio_channels.is_err()
        {
            return Err(Error::StatusNotAvailable);
        }

        let unit = unit.unwrap_or_default();
        let destination = destinations
            .unwrap_or_default()
            .into_iter()
            .find(|destination| destination.selected)
            .map(|destination| destination.name);
        let video = video.ok();

        Ok(SafeUnitConfig {
            name: unit.name,
            model: unit.model,
            status: unit.status,
            destination,
            resolution: video.as_ref().and_then(|video| video.resolution.clone()),
            bitrate: video.and_then(|video| video.bitrate),
            delay: delay.ok().map(|delay| delay.delay),
            audio_channels: audio_channels.ok(),
            firmware: unit.sw_version,
        })
    }

    pub async fn get_interfaces(&self, boss_id: &str) -> Result<Vec<Interface>, Error> {
        let res = self
            .send_request(
                Method::GET,
                &format!("{}/units/{}/status/interfaces", LIVEU_API, &boss_id),
                None::<()>,
            )
            .await?;

        match res.status() {
            StatusCode::OK => Ok(res.json().await?),
            StatusCode::NO_CONTENT => Ok(vec![]),
            _ => Err(Error::NoUnitsFound),
        }
    }

    pub async fn get_battery(&self, boss_id: &str) -> Result<Battery, Error> {
        let res = self
            .send_request(
                Method::GET,
                &format!("{}/units/{}/status/battery", LIVEU_API, &boss_id),
                None::<()>,
            )
            .await?;

        match res.status() {
            StatusCode::OK => Ok(res.json().await?),
            _ => Err(Error::StatusNotAvailable),
        }
    }

    pub async fn get_video(&self, boss_id: &str) -> Result<Video, Error> {
        let res = self
            .send_request(
                Method::GET,
                &format!("{}/units/{}/status/video", LIVEU_API, &boss_id),
                None::<()>,
            )
            .await?;

        match res.status() {
            StatusCode::OK => Ok(res.json().await?),
            _ => Err(Error::StatusNotAvailable),
        }
    }

    pub async fn is_idle(&self, boss_id: &str) -> bool {
        let video = match self.get_video(boss_id).await {
            Ok(v) => v,
            Err(_) => return false,
        };

        if video.resolution.is_some() && video.bitrate.is_none() {
            return true;
        }

        false
    }

    pub async fn is_streaming(&self, boss_id: &str) -> bool {
        let video = match self.get_video(boss_id).await {
            Ok(v) => v,
            Err(_) => return false,
        };

        if video.bitrate.is_some() {
            return true;
        }

        false
    }

    pub async fn start_stream(&self, boss_id: &str) -> Result<(), Error> {
        let mut map = HashMap::new();
        map.insert("unit_id", boss_id);

        let res = self
            .send_request(
                Method::POST,
                &format!("{}/units/{}/stream", LIVEU_API, &boss_id),
                Some(map),
            )
            .await?;

        match res.status() {
            StatusCode::CREATED => Ok(()),
            _ => Err(Error::StatusNotAvailable),
        }
    }

    pub async fn stop_stream(&self, boss_id: &str) -> Result<(), Error> {
        let res = self
            .send_request(
                Method::DELETE,
                &format!("{}/units/{}/stream", LIVEU_API, &boss_id),
                None::<()>,
            )
            .await?;

        match res.status() {
            StatusCode::NO_CONTENT => Ok(()),
            _ => Err(Error::StatusNotAvailable),
        }
    }

    pub async fn reboot_unit(&self, boss_id: &str) -> Result<(), Error> {
        let res = self
            .send_request(
                Method::POST,
                &format!("{}/units/{}/reboot", LIVEU_API_V2, &boss_id),
                None::<()>,
            )
            .await?;

        match res.status() {
            StatusCode::NO_CONTENT => Ok(()),
            _ => Err(Error::StatusNotAvailable),
        }
    }

    pub async fn get_delay(&self, boss_id: &str) -> Result<Delay, Error> {
        let res = self
            .send_request(
                Method::GET,
                &format!("{}/units/{}?fields=delay", LIVEU_API, &boss_id),
                None::<()>,
            )
            .await?;

        match res.status() {
            StatusCode::OK => {
                let value: serde_json::Value = res.json().await?;
                let delay = value["data"]["unit"]["delay"]
                    .as_u64()
                    .ok_or(Error::StatusNotAvailable)?;
                Ok(Delay { delay })
            }
            _ => Err(Error::StatusNotAvailable),
        }
    }

    pub async fn set_delay(&self, boss_id: &str, delay: u64) -> Result<(), Error> {
        let res = self
            .send_request(
                Method::PUT,
                &format!("{}/units/{}/delay", LIVEU_API, &boss_id),
                Some(DelayReq {
                    unit: Delay { delay },
                }),
            )
            .await?;

        match res.status() {
            StatusCode::NO_CONTENT => Ok(()),
            _ => Err(Error::StatusNotAvailable),
        }
    }

    /// Gets the location of the boss_id in the inventories
    pub fn get_boss_id_location(inventories: &Inventories) -> usize {
        let size = inventories.units.len();

        if size == 0 {
            panic!("No units found!");
        }

        if size > 1 {
            println!("Found {} units!\n", size);

            for (pos, unit) in inventories.units.iter().enumerate() {
                println!("({}) {}", pos + 1, unit.name);
            }

            let inp = input()
                .msg("\nPlease enter which one you want to use (1): ")
                .inside_err(
                    1..=size,
                    format!("Please enter a number between 1 and {}: ", size),
                )
                .err("That does not look like a number. Please try again:")
                .default(1)
                .get();

            return inp - 1;
        }

        0
    }

    pub async fn get_unit_custom_names(
        &self,
        boss_id: &str,
        custom_names: Option<config::CustomUnitNames>,
    ) -> Result<Vec<Interface>, Error> {
        let custom_names = custom_names.unwrap_or_default();

        Ok(self
            .get_interfaces(boss_id)
            .await?
            .into_iter()
            .filter(|x| x.connected)
            .map(|x| Self::change_interface_name_to_custom(x, &custom_names))
            .collect())
    }

    pub async fn get_all_unit_custom_names(
        &self,
        boss_id: &str,
        custom_names: Option<config::CustomUnitNames>,
    ) -> Result<Vec<Interface>, Error> {
        let custom_names = custom_names.unwrap_or_default();

        Ok(self
            .get_interfaces(boss_id)
            .await?
            .into_iter()
            .map(|interface| Self::change_interface_name_to_custom(interface, &custom_names))
            .collect())
    }

    fn change_interface_name_to_custom(
        mut interface: Interface,
        custom_names: &config::CustomUnitNames,
    ) -> Interface {
        match interface.port.as_ref() {
            "eth0" => {
                interface.port = custom_names.ethernet.to_string();
            }
            "wlan0" => {
                interface.port = custom_names.wifi.to_string();
            }
            "0" => {
                interface.port = custom_names.sim1.to_string();
            }
            "1" => {
                interface.port = custom_names.sim2.to_string();
            }
            "2" => {
                interface.port = custom_names.usb1.to_string();
            }
            "3" => {
                interface.port = custom_names.usb2.to_string();
            }
            _ => {}
        }

        interface
    }
}

fn nested_value<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(value, |current, key| current.get(key))
}

fn value_string(value: &Value) -> Option<String> {
    let value = match value {
        Value::String(value) => value.trim().to_string(),
        Value::Number(value) => value.to_string(),
        _ => return None,
    };

    (!value.is_empty()).then_some(value)
}

fn field_string(value: &Value, fields: &[&str]) -> Option<String> {
    fields
        .iter()
        .find_map(|field| value.get(field).and_then(value_string))
}

fn field_bool(value: &Value, fields: &[&str]) -> Option<bool> {
    fields.iter().find_map(|field| {
        value.get(field).and_then(|value| match value {
            Value::Bool(value) => Some(*value),
            Value::String(value) if value.eq_ignore_ascii_case("true") => Some(true),
            Value::String(value) if value.eq_ignore_ascii_case("false") => Some(false),
            _ => None,
        })
    })
}

fn stream_tools_zone(selected_channel: Option<&str>) -> &'static str {
    match selected_channel.unwrap_or_default() {
        "Cloud-Service"
        | "NA_Montreal"
        | "SA_Central_SanPaulo"
        | "Brazil_Sao_Paulo"
        | "Canada_Montreal"
        | "USA_Ashburn" => "us-vir-4",
        "Cloud_Service_US_SJ" | "Canada_Toronto" | "USA_San_Jose" => "us-ore-4",
        "Cloud_Service_APAC" | "AP_Tokyo" | "Japan_Osaka" | "Japan_Tokyo" => "ap-tok-1",
        "Cloud_Service_Austra"
        | "AU_Melbourne"
        | "Australia_Melbourne"
        | "Australia_Sydney"
        | "Singapore" => "ap-syd-1",
        "AP_India_Mumbai" | "India_Mumbai" | "ME_Dubai" | "UAE_Dubai" => "ap-mum-1",
        "Cloud_Service_EMEA" | "EMEA_RU_Russia" | "EU_Frankfurt" | "EU_London"
        | "Germany_Frankfurt" | "Italy_Milan" | "UK_London" | "UK_Newport" => "eu-irl-4",
        _ => "eu-irl-4",
    }
}

#[cfg(test)]
mod tests {
    use super::{field_bool, field_string, nested_value, stream_tools_zone};
    use serde_json::json;

    #[test]
    fn extracts_only_safe_scalar_fields() {
        let value = json!({
            "name": " Twitch ",
            "fps": 59.94,
            "toggle_state": "true",
            "secret": {"stream_key": "do-not-expose"}
        });

        assert_eq!(field_string(&value, &["name"]), Some("Twitch".to_string()));
        assert_eq!(field_string(&value, &["fps"]), Some("59.94".to_string()));
        assert_eq!(field_bool(&value, &["toggle_state"]), Some(true));
        assert_eq!(field_string(&value, &["secret"]), None);
    }

    #[test]
    fn reads_audio_channel_count_from_configuration() {
        let value = json!({
            "singleCamera": {
                "streamingInformation": {
                    "sdi0": {"audio": {"numberOfChannels": "4"}}
                }
            }
        });

        assert_eq!(
            nested_value(
                &value,
                &[
                    "singleCamera",
                    "streamingInformation",
                    "sdi0",
                    "audio",
                    "numberOfChannels"
                ]
            )
            .and_then(super::value_string),
            Some("4".to_string())
        );
    }

    #[test]
    fn maps_solo_channels_to_stream_tools_zones() {
        assert_eq!(stream_tools_zone(Some("Cloud-Service")), "us-vir-4");
        assert_eq!(stream_tools_zone(Some("Cloud_Service_US_SJ")), "us-ore-4");
        assert_eq!(stream_tools_zone(Some("Cloud_Service_Austra")), "ap-syd-1");
        assert_eq!(stream_tools_zone(Some("Canada_Toronto")), "us-ore-4");
        assert_eq!(stream_tools_zone(Some("Singapore")), "ap-syd-1");
        assert_eq!(stream_tools_zone(Some("Italy_Milan")), "eu-irl-4");
        assert_eq!(stream_tools_zone(None), "eu-irl-4");
    }
}
