use crate::{
    config,
    error::Error,
    liveu::{self, Liveu},
    nginx,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use twitch_irc::{
    login::StaticLoginCredentials,
    message,
    transport::tcp::{TCPTransport, TLS},
    ClientConfig, TwitchIRCClient,
};

const OFFLINE_MSG: &str = "LiveU Offline :(";

pub struct Twitch {
    client: TwitchIRCClient<TCPTransport<TLS>, StaticLoginCredentials>,
    liveu: Liveu,
    liveu_boss_id: String,
    config: config::Config,
    timeout: Arc<AtomicBool>,
}

impl Twitch {
    pub fn run(
        config: config::Config,
        liveu: Liveu,
        liveu_boss_id: String,
    ) -> (
        TwitchIRCClient<TCPTransport<TLS>, StaticLoginCredentials>,
        tokio::task::JoinHandle<()>,
    ) {
        let config::Twitch {
            bot_username,
            bot_oauth,
            channel,
            mod_only,
            ..
        } = &config.twitch;

        let username = bot_username.to_lowercase();
        let channel = channel.to_lowercase();
        let mut oauth = bot_oauth.to_owned();

        if let Some(strip_oauth) = oauth.strip_prefix("oauth:") {
            oauth = strip_oauth.to_string();
        }

        let twitch_credentials = StaticLoginCredentials::new(username, Some(oauth));
        let twitch_config = ClientConfig::new_simple(twitch_credentials);
        let (mut incoming_messages, client) =
            TwitchIRCClient::<TCPTransport<TLS>, StaticLoginCredentials>::new(twitch_config);

        client.join(channel);

        let mod_only = mod_only.to_owned();
        let client_clone = client.clone();
        let join_handler = tokio::spawn(async move {
            let t = Self {
                client: client_clone,
                liveu,
                liveu_boss_id,
                config,
                timeout: Arc::new(AtomicBool::new(false)),
            };

            while let Some(message) = incoming_messages.recv().await {
                t.handle_chat(message, &mod_only).await;
            }
        });

        (client, join_handler)
    }

    async fn handle_chat(&self, message: message::ServerMessage, mod_only: &bool) {
        let timeout = self.timeout.clone();
        if timeout.load(Ordering::Acquire) {
            return;
        }

        match message {
            message::ServerMessage::Notice(msg) => {
                if msg.message_text == "Login authentication failed" {
                    panic!("Twitch authentication failed");
                }
            }
            message::ServerMessage::Privmsg(msg) => {
                let is_owner = msg.badges.contains(&twitch_irc::message::Badge {
                    name: "broadcaster".to_string(),
                    version: "1".to_string(),
                });

                let is_mod = msg.badges.contains(&twitch_irc::message::Badge {
                    name: "moderator".to_string(),
                    version: "1".to_string(),
                });

                let mut user_has_permission = false;

                if let Some(users) = &self.config.twitch.admin_users {
                    for user in users {
                        if user.to_lowercase() == msg.sender.login {
                            user_has_permission = true;
                            break;
                        }
                    }
                };

                if *mod_only && !(is_owner || is_mod || user_has_permission) {
                    return;
                }

                let command = msg
                    .message_text
                    .split_ascii_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string();

                let command = self.get_command(command);

                if command == Command::Unknown {
                    return;
                }

                let cooldown = self.config.commands.cooldown;

                tokio::spawn(async move {
                    timeout.store(true, Ordering::Release);
                    tokio::time::sleep(tokio::time::Duration::from_secs(cooldown as u64)).await;
                    timeout.store(false, Ordering::Release);
                });

                let res = if matches!(
                    command,
                    Command::Stats
                        | Command::Battery
                        | Command::Destinations
                        | Command::Firmware
                        | Command::Interfaces
                        | Command::StreamTools
                        | Command::Config
                ) {
                    self.handle_non_permission_commands(command).await
                } else {
                    if !(is_owner || user_has_permission) {
                        return;
                    }

                    self.handle_permission_commands(command, msg.channel_login.to_owned())
                        .await
                };

                if let Ok(res) = res {
                    let _ = self.client.say(msg.channel_login.to_owned(), res).await;
                }
            }
            _ => {}
        };
    }

    async fn handle_non_permission_commands(&self, command: Command) -> Result<String, Error> {
        match command {
            Command::Stats => self.generate_liveu_modems_message().await,
            Command::Battery => self.generate_liveu_battery_message().await,
            Command::Destinations => self.generate_liveu_destinations_message().await,
            Command::Firmware => self.generate_liveu_firmware_message().await,
            Command::Interfaces => self.generate_liveu_interfaces_message().await,
            Command::StreamTools => self.generate_stream_tools_message().await,
            Command::Config => self.generate_liveu_config_message().await,
            _ => unreachable!(),
        }
    }

    async fn handle_permission_commands(
        &self,
        command: Command,
        channel: String,
    ) -> Result<String, Error> {
        match command {
            Command::Start => self.generate_liveu_start_message(channel).await,
            Command::Stop => self.generate_liveu_stop_message(channel).await,
            Command::Restart => self.generate_liveu_restart_message(channel).await,
            Command::Reboot => self.generate_liveu_reboot_message(channel).await,
            Command::Delay => self.toggle_delay(channel).await,
            _ => unreachable!(),
        }
    }

    // TODO: This needs a refactor
    fn get_command(&self, command: String) -> Command {
        let config::Commands {
            stats,
            battery,
            destinations,
            firmware,
            interfaces,
            streamtools,
            config,
            start,
            stop,
            restart,
            reboot,
            delay,
            ..
        } = &self.config.commands;

        if stats.contains(&command) {
            return Command::Stats;
        }

        if battery.contains(&command) {
            return Command::Battery;
        }

        if destinations == &command {
            return Command::Destinations;
        }

        if firmware == &command {
            return Command::Firmware;
        }

        if interfaces == &command {
            return Command::Interfaces;
        }

        if streamtools == &command {
            return Command::StreamTools;
        }

        if config == &command {
            return Command::Config;
        }

        if start == &command {
            return Command::Start;
        }

        if stop == &command {
            return Command::Stop;
        }

        if restart == &command {
            return Command::Restart;
        }

        if reboot == &command {
            return Command::Reboot;
        }

        if delay == &command {
            return Command::Delay;
        }

        Command::Unknown
    }

    async fn generate_liveu_modems_message(&self) -> Result<String, Error> {
        let interfaces: Vec<liveu::Interface> = self
            .liveu
            .get_unit_custom_names(&self.liveu_boss_id, self.config.custom_port_names.clone())
            .await?;

        if interfaces.is_empty() {
            return Ok(OFFLINE_MSG.to_string());
        }

        let mut message = String::new();
        let mut total_bitrate = 0;

        for interface in interfaces.iter() {
            message = message
                + &format!(
                    "{}: {} Kbps{}{}, ",
                    interface.port,
                    interface.uplink_kbps,
                    if !interface.technology.is_empty() {
                        format!(" ({})", &interface.technology)
                    } else {
                        "".to_string()
                    },
                    if interface.is_currently_roaming {
                        " roaming"
                    } else {
                        ""
                    }
                );
            total_bitrate += interface.uplink_kbps;
        }

        if total_bitrate == 0 {
            return Ok("LiveU Online and Ready".to_string());
        }

        message += &format!("Total LRT: {} Kbps", total_bitrate);

        if let Some(rtmp) = &self.config.rtmp {
            if let Ok(Some(bitrate)) = nginx::get_rtmp_bitrate(rtmp).await {
                message += &format!(", RTMP: {} Kbps", bitrate);
            };
        }

        Ok(message)
    }

    async fn generate_liveu_battery_message(&self) -> Result<String, Error> {
        let battery = match self.liveu.get_battery(&self.liveu_boss_id).await {
            Ok(b) => b,
            Err(_) => return Ok(OFFLINE_MSG.to_string()),
        };

        let estimated_battery_time = {
            if battery.run_time_to_empty != 0 && battery.discharging {
                let hours = battery.run_time_to_empty / 60;
                let minutes = battery.run_time_to_empty % 60;
                let mut time_string = String::new();

                if hours != 0 {
                    time_string += &format!("{}h", hours);
                }

                time_string += &format!(" {}m", minutes);
                format!("Estimated battery time: {}", time_string)
            } else {
                "".to_string()
            }
        };

        let charging = {
            if battery.charging {
                "charging".to_string()
            } else if battery.percentage == 100 {
                let mut s = "fully charged".to_string();

                if battery.connected {
                    s += ", connected"
                }

                s
            } else if battery.percentage < 100 && !battery.charging && !battery.discharging {
                "too hot to charge".to_string()
            } else {
                "not charging".to_string()
            }
        };

        let message = format!(
            "LiveU Internal Battery: {}% {} {}",
            battery.percentage, charging, estimated_battery_time
        );

        Ok(message)
    }

    async fn generate_liveu_destinations_message(&self) -> Result<String, Error> {
        Ok(
            match self.liveu.get_destinations(&self.liveu_boss_id).await {
                Ok(destinations) => format_destinations(&destinations),
                Err(_) => "LiveU destinations are temporarily unavailable".to_string(),
            },
        )
    }

    async fn generate_liveu_firmware_message(&self) -> Result<String, Error> {
        Ok(
            match self.liveu.get_firmware_info(&self.liveu_boss_id).await {
                Ok(firmware) => format_firmware(&firmware),
                Err(_) => "LiveU firmware information is temporarily unavailable".to_string(),
            },
        )
    }

    async fn generate_liveu_interfaces_message(&self) -> Result<String, Error> {
        Ok(
            match self
                .liveu
                .get_all_unit_custom_names(
                    &self.liveu_boss_id,
                    self.config.custom_port_names.clone(),
                )
                .await
            {
                Ok(interfaces) => format_interfaces(&interfaces),
                Err(_) => "LiveU interfaces are temporarily unavailable".to_string(),
            },
        )
    }

    async fn generate_stream_tools_message(&self) -> Result<String, Error> {
        Ok(
            match self.liveu.get_stream_tools(&self.liveu_boss_id).await {
                Ok(stream_tools) => format_stream_tools(&stream_tools),
                Err(_) => "Stream Tools status is temporarily unavailable".to_string(),
            },
        )
    }

    async fn generate_liveu_config_message(&self) -> Result<String, Error> {
        Ok(
            match self.liveu.get_safe_unit_config(&self.liveu_boss_id).await {
                Ok(config) => format_safe_config(&config),
                Err(_) => "LiveU configuration is temporarily unavailable".to_string(),
            },
        )
    }

    async fn generate_liveu_start_message(&self, channel: String) -> Result<String, Error> {
        let video = self.liveu.get_video(&self.liveu_boss_id).await;

        let video = match video {
            Ok(video) => video,
            Err(_) => return Ok(OFFLINE_MSG.to_string()),
        };

        if video.resolution.is_none() {
            return Ok("LiveU no camera plugged in".to_string());
        }

        if video.bitrate.is_some() {
            return Ok("LiveU already streaming".to_string());
        }

        if self.liveu.start_stream(&self.liveu_boss_id).await.is_err() {
            return Ok("LiveU request error".to_string());
        };

        let confirm = DataUsedInThread {
            chat: self.client.clone(),
            liveu: self.liveu.clone(),
            boss_id: self.liveu_boss_id.to_owned(),
            channel,
        };

        tokio::spawn(async move {
            confirm
                .confirm_action(15, true, "started".to_string(), "starting".to_string())
                .await
        });

        Ok("LiveU starting stream".to_string())
    }

    async fn generate_liveu_stop_message(&self, channel: String) -> Result<String, Error> {
        if !self.liveu.is_streaming(&self.liveu_boss_id).await {
            return Ok("LiveU already stopped".to_string());
        }

        if self.liveu.stop_stream(&self.liveu_boss_id).await.is_err() {
            return Ok("LiveU request error".to_string());
        };

        let confirm = DataUsedInThread {
            chat: self.client.clone(),
            liveu: self.liveu.clone(),
            boss_id: self.liveu_boss_id.to_owned(),
            channel,
        };

        tokio::spawn(async move {
            confirm
                .confirm_action(10, false, "stopped".to_string(), "stopping".to_string())
                .await
        });

        Ok("LiveU stopping stream".to_string())
    }

    async fn generate_liveu_restart_message(&self, channel: String) -> Result<String, Error> {
        if !self.liveu.is_streaming(&self.liveu_boss_id).await {
            return Ok("LiveU not streaming".to_string());
        }

        let msg = "LiveU stream restarting".to_string();
        let _ = self.client.say(channel.to_owned(), msg).await;

        self.generate_liveu_stop_message(channel.to_owned()).await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(4)).await;
        self.generate_liveu_start_message(channel.to_owned())
            .await?;

        Ok(String::new())
    }

    async fn generate_liveu_reboot_message(&self, channel: String) -> Result<String, Error> {
        let is_streaming = self.liveu.is_streaming(&self.liveu_boss_id).await;

        let msg = "LiveU Rebooting, please wait approximately 2-3 minutes".to_string();
        let _ = self.client.say(channel.to_owned(), msg).await;

        if is_streaming {
            self.generate_liveu_stop_message(channel.to_owned()).await?;
            tokio::time::sleep(tokio::time::Duration::from_secs(4)).await;
        }

        self.liveu.reboot_unit(&self.liveu_boss_id).await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

        let mut attempts = 0;
        let max_attempts = 20;

        while !self.liveu.is_idle(&self.liveu_boss_id).await && attempts != max_attempts {
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            attempts += 1;
        }

        if attempts == max_attempts {
            return Ok("LiveU took too long to reboot".to_string());
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

        if is_streaming {
            self.generate_liveu_start_message(channel.to_owned())
                .await?;
            return Ok(String::new());
        }

        Ok("LiveU rebooted successfully".to_string())
    }

    async fn toggle_delay(&self, channel: String) -> Result<String, Error> {
        let is_streaming = self.liveu.is_streaming(&self.liveu_boss_id).await;

        if is_streaming {
            self.generate_liveu_stop_message(channel.to_owned()).await?;
            tokio::time::sleep(tokio::time::Duration::from_secs(4)).await;
        }

        let current_delay = self.liveu.get_delay(&self.liveu_boss_id).await?;
        let delay = if current_delay.delay == 1000 {
            (5000, "LiveU high resiliency mode")
        } else {
            (1000, "LiveU low delay mode")
        };

        self.liveu.set_delay(&self.liveu_boss_id, delay.0).await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        if is_streaming {
            self.generate_liveu_start_message(channel.to_owned())
                .await?;
        }

        Ok(delay.1.to_string())
    }
}

const MAX_CHAT_MESSAGE_CHARS: usize = 450;

fn clean_chat_value(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(80)
        .collect()
}

fn truncate_chat_message(message: String) -> String {
    if message.chars().count() <= MAX_CHAT_MESSAGE_CHARS {
        return message;
    }

    let mut shortened: String = message
        .chars()
        .take(MAX_CHAT_MESSAGE_CHARS.saturating_sub(3))
        .collect();
    shortened.push_str("...");
    shortened
}

fn format_kbps(kbps: u32) -> String {
    if kbps >= 1000 {
        format!("{:.1} Mbps", kbps as f64 / 1000.0)
    } else {
        format!("{} Kbps", kbps)
    }
}

fn format_destinations(destinations: &[liveu::Destination]) -> String {
    if destinations.is_empty() {
        return "LiveU destinations: none configured".to_string();
    }

    let visible = destinations
        .iter()
        .take(6)
        .map(|destination| {
            format!(
                "{}{}",
                clean_chat_value(&destination.name),
                if destination.selected {
                    " [selected]"
                } else {
                    ""
                }
            )
        })
        .collect::<Vec<_>>()
        .join(" • ");
    let remaining = destinations.len().saturating_sub(6);
    let suffix = if remaining > 0 {
        format!(" • +{} more", remaining)
    } else {
        String::new()
    };

    truncate_chat_message(format!(
        "LiveU destinations: {}{} — {} configured",
        visible,
        suffix,
        destinations.len()
    ))
}

fn format_firmware(firmware: &liveu::FirmwareInfo) -> String {
    let current = firmware.current.as_deref().unwrap_or("unknown");
    let alternatives: Vec<_> = firmware
        .available
        .iter()
        .filter(|version| Some(version.as_str()) != firmware.current.as_deref())
        .take(4)
        .map(|version| clean_chat_value(version))
        .collect();

    if alternatives.is_empty() {
        return format!(
            "LiveU firmware: current {} • No alternate versions reported",
            clean_chat_value(current)
        );
    }

    let hidden = firmware
        .available
        .iter()
        .filter(|version| Some(version.as_str()) != firmware.current.as_deref())
        .count()
        .saturating_sub(alternatives.len());
    let suffix = if hidden > 0 {
        format!(" +{} more", hidden)
    } else {
        String::new()
    };

    truncate_chat_message(format!(
        "LiveU firmware: current {} • Selectable: {}{}",
        clean_chat_value(current),
        alternatives.join(", "),
        suffix
    ))
}

fn format_interfaces(interfaces: &[liveu::Interface]) -> String {
    if interfaces.is_empty() {
        return "LiveU carriers: unavailable".to_string();
    }

    let mut total_kbps = 0;
    let mut parts = Vec::new();
    for interface in interfaces.iter().filter(|interface| interface.connected) {
        total_kbps += interface.uplink_kbps;
        let carrier = if interface.name.trim().is_empty()
            || interface_labels_match(&interface.name, &interface.port)
        {
            String::new()
        } else {
            format!(" {}", clean_chat_value(&interface.name))
        };
        let technology = if interface.technology.trim().is_empty() {
            String::new()
        } else if interface_labels_match(&interface.technology, &interface.name)
            || interface_labels_match(&interface.technology, &interface.port)
        {
            String::new()
        } else {
            format!(" {}", clean_chat_value(&interface.technology))
        };
        parts.push(format!(
            "{}{} {}{}",
            clean_chat_value(&interface.port),
            carrier,
            format_kbps(interface.uplink_kbps),
            technology
        ));
    }

    for interface in interfaces.iter().filter(|interface| !interface.connected) {
        let carrier = if interface.name.trim().is_empty()
            || interface_labels_match(&interface.name, &interface.port)
        {
            String::new()
        } else {
            format!(" {}", clean_chat_value(&interface.name))
        };
        parts.push(format!(
            "{}{} {}",
            clean_chat_value(&interface.port),
            carrier,
            if interface.enabled {
                "disconnected"
            } else {
                "off"
            }
        ));
    }

    if total_kbps > 0 {
        parts.push(format!("Total {}", format_kbps(total_kbps)));
    }

    truncate_chat_message(format!("LiveU carriers: {}", parts.join(" • ")))
}

fn interface_labels_match(left: &str, right: &str) -> bool {
    let normalize = |value: &str| {
        value
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .map(|character| character.to_ascii_lowercase())
            .collect::<String>()
    };

    normalize(left) == normalize(right)
}

fn format_video_bitrate(value: &str) -> String {
    value
        .parse::<f64>()
        .ok()
        .map(|value| {
            if value >= 1000.0 {
                format!("{:.1} Mbps", value / 1000.0)
            } else {
                format!("{} Kbps", value)
            }
        })
        .unwrap_or_else(|| clean_chat_value(value))
}

fn format_audio_bitrate(value: &str) -> String {
    value
        .parse::<f64>()
        .ok()
        .map(|value| {
            if value >= 1000.0 {
                format!("{:.0} Kbps", value / 1000.0)
            } else {
                format!("{} Kbps", value)
            }
        })
        .unwrap_or_else(|| clean_chat_value(value))
}

fn friendly_zone(zone: &str) -> &str {
    match zone {
        "us-vir-4" => "US Virginia",
        "us-ore-4" => "US Oregon",
        "eu-irl-4" | "eu-irl-1" => "EU Ireland",
        "ap-syd-1" => "AP Sydney",
        "ap-tok-1" => "AP Tokyo",
        "ap-mum-1" => "AP Mumbai",
        _ => zone,
    }
}

fn format_stream_tools(stream_tools: &liveu::StreamToolsSummary) -> String {
    if !stream_tools.enabled {
        return "Stream Tools: not enabled for this Solo account or unit".to_string();
    }

    let mut parts = vec![if stream_tools.active {
        "active".to_string()
    } else {
        "inactive".to_string()
    }];
    if let Some(zone) = &stream_tools.zone {
        parts.push(friendly_zone(zone).to_string());
    }
    if let Some(profile) = &stream_tools.profile {
        parts.push(clean_chat_value(profile));
    }
    if let Some(fps) = &stream_tools.fps {
        parts.push(format!("{} fps", clean_chat_value(fps)));
    }
    if let Some(bitrate) = &stream_tools.video_bitrate {
        parts.push(format!("Video {}", format_video_bitrate(bitrate)));
    }
    if let Some(bitrate) = &stream_tools.audio_bitrate {
        parts.push(format!("Audio {}", format_audio_bitrate(bitrate)));
    }
    if stream_tools.destinations.is_empty() {
        parts.push("No destinations".to_string());
    } else {
        parts.push(format!(
            "Destinations {} ({}/3)",
            stream_tools
                .destinations
                .iter()
                .take(3)
                .map(|name| clean_chat_value(name))
                .collect::<Vec<_>>()
                .join(" + "),
            stream_tools.destinations.len().min(3)
        ));
    }
    parts.push(format!(
        "Logo {}",
        if stream_tools.logo_enabled {
            "on"
        } else {
            "off"
        }
    ));
    parts.push(format!(
        "Slate {}",
        if stream_tools.fallback_slate_ready {
            "ready"
        } else {
            "missing"
        }
    ));

    truncate_chat_message(format!("Stream Tools: {}", parts.join(" • ")))
}

fn format_safe_config(config: &liveu::SafeUnitConfig) -> String {
    let mut parts = Vec::new();
    if let Some(name) = &config.name {
        parts.push(clean_chat_value(name));
    }
    if let Some(model) = &config.model {
        parts.push(clean_chat_value(model));
    }
    if let Some(status) = &config.status {
        parts.push(clean_chat_value(status));
    }
    if let Some(destination) = &config.destination {
        parts.push(format!("Destination {}", clean_chat_value(destination)));
    }
    if let Some(resolution) = &config.resolution {
        let video = config
            .bitrate
            .map(|bitrate| {
                format!(
                    "{} @ {}",
                    clean_chat_value(resolution),
                    format_kbps(bitrate)
                )
            })
            .unwrap_or_else(|| clean_chat_value(resolution));
        parts.push(video);
    }
    if let Some(delay) = config.delay {
        parts.push(match delay {
            1000 => "Low Delay 1s".to_string(),
            5000 => "High Resiliency 5s".to_string(),
            delay => format!("Delay {:.1}s", delay as f64 / 1000.0),
        });
    }
    if let Some(channels) = &config.audio_channels {
        parts.push(format!("Audio {}ch", clean_chat_value(channels)));
    }
    if let Some(firmware) = &config.firmware {
        parts.push(format!("Firmware {}", clean_chat_value(firmware)));
    }

    if parts.is_empty() {
        "LiveU configuration is unavailable".to_string()
    } else {
        truncate_chat_message(format!("LiveU config: {}", parts.join(" • ")))
    }
}

#[derive(PartialEq, Eq)]
enum Command {
    Stats,
    Battery,
    Destinations,
    Firmware,
    Interfaces,
    StreamTools,
    Config,
    Start,
    Stop,
    Restart,
    Reboot,
    Delay,
    Unknown,
}

struct DataUsedInThread {
    chat: TwitchIRCClient<TCPTransport<TLS>, StaticLoginCredentials>,
    liveu: Liveu,
    boss_id: String,
    channel: String,
}

impl DataUsedInThread {
    async fn confirm_action(
        &self,
        max_attempts: u8,
        should_have_bitrate: bool,
        success: String,
        not_success: String,
    ) {
        let mut attempts = 0;

        while attempts != max_attempts {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

            let video = self.liveu.get_video(&self.boss_id).await;

            if let Ok(video) = video {
                if video.bitrate.is_some() == should_have_bitrate {
                    break;
                }
            }

            attempts += 1;
        }

        if attempts == max_attempts {
            let msg = format!(
                "LiveU {} stream took too long might not have worked",
                not_success
            );
            let _ = self.chat.say(self.channel.to_owned(), msg).await;

            return;
        }

        let msg = format!("LiveU streaming {} successfully", success);
        let _ = self.chat.say(self.channel.to_owned(), msg).await;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        format_destinations, format_firmware, format_interfaces, format_safe_config,
        format_stream_tools, MAX_CHAT_MESSAGE_CHARS,
    };
    use crate::liveu::{Destination, FirmwareInfo, Interface, SafeUnitConfig, StreamToolsSummary};

    #[test]
    fn formats_destinations_and_marks_the_selected_one() {
        let destinations = vec![
            Destination {
                id: "one".to_string(),
                name: "Twitch".to_string(),
                provider: Some("Generic".to_string()),
                selected: true,
            },
            Destination {
                id: "two".to_string(),
                name: "YouTube".to_string(),
                provider: Some("YouTube".to_string()),
                selected: false,
            },
        ];

        assert_eq!(
            format_destinations(&destinations),
            "LiveU destinations: Twitch [selected] • YouTube — 2 configured"
        );
    }

    #[test]
    fn firmware_output_does_not_claim_an_update_is_required() {
        let firmware = FirmwareInfo {
            current: Some("v9.2.1".to_string()),
            available: vec!["v9.1.0".to_string(), "v9.2.1".to_string()],
        };

        assert_eq!(
            format_firmware(&firmware),
            "LiveU firmware: current v9.2.1 • Selectable: v9.1.0"
        );
    }

    #[test]
    fn carrier_output_includes_carrier_names_without_duplicate_technology() {
        let interfaces = vec![
            Interface {
                connected: true,
                name: "Ethernet".to_string(),
                downlink_kbps: 0,
                uplink_kbps: 8200,
                enabled: true,
                port: "ETH".to_string(),
                technology: "Ethernet".to_string(),
                up_signal_quality: 0,
                down_signal_quality: 0,
                active_sim: None,
                is_currently_roaming: false,
                kbps: 0,
                signal_quality: 0,
            },
            Interface {
                connected: true,
                name: "AT&T".to_string(),
                downlink_kbps: 0,
                uplink_kbps: 1500,
                enabled: true,
                port: "USB1".to_string(),
                technology: "4G".to_string(),
                up_signal_quality: 0,
                down_signal_quality: 0,
                active_sim: None,
                is_currently_roaming: true,
                kbps: 0,
                signal_quality: 0,
            },
            Interface {
                connected: false,
                name: "Wi-Fi".to_string(),
                downlink_kbps: 0,
                uplink_kbps: 0,
                enabled: true,
                port: "WiFi".to_string(),
                technology: String::new(),
                up_signal_quality: 0,
                down_signal_quality: 0,
                active_sim: None,
                is_currently_roaming: false,
                kbps: 0,
                signal_quality: 0,
            },
        ];

        assert_eq!(
            format_interfaces(&interfaces),
            "LiveU carriers: ETH Ethernet 8.2 Mbps • USB1 AT&T 1.5 Mbps 4G • WiFi disconnected • Total 9.7 Mbps"
        );
    }

    #[test]
    fn stream_tools_output_contains_only_status_fields() {
        let stream_tools = StreamToolsSummary {
            enabled: true,
            active: true,
            zone: Some("us-ore-4".to_string()),
            profile: Some("1920 x 1080 Widescreen (16:9)".to_string()),
            fps: Some("60".to_string()),
            video_bitrate: Some("6000".to_string()),
            audio_bitrate: Some("128000".to_string()),
            destinations: vec!["Twitch".to_string(), "YouTube".to_string()],
            logo_enabled: true,
            fallback_slate_ready: true,
        };
        let output = format_stream_tools(&stream_tools);

        assert!(output.contains("Stream Tools: active"));
        assert!(output.contains("US Oregon"));
        assert!(output.contains("Video 6.0 Mbps"));
        assert!(output.contains("Audio 128 Kbps"));
        assert!(output.contains("Twitch + YouTube (2/3)"));
        assert!(output.chars().count() <= MAX_CHAT_MESSAGE_CHARS);
    }

    #[test]
    fn safe_config_is_a_summary_not_a_raw_dump() {
        let config = SafeUnitConfig {
            name: Some("Travel Solo".to_string()),
            model: Some("Solo PRO".to_string()),
            status: Some("streaming".to_string()),
            destination: Some("Twitch".to_string()),
            resolution: Some("1920x1080".to_string()),
            bitrate: Some(6000),
            delay: Some(1000),
            audio_channels: Some("2".to_string()),
            firmware: Some("v9.2.1".to_string()),
        };
        let output = format_safe_config(&config);

        assert_eq!(
            output,
            "LiveU config: Travel Solo • Solo PRO • streaming • Destination Twitch • 1920x1080 @ 6.0 Mbps • Low Delay 1s • Audio 2ch • Firmware v9.2.1"
        );
        assert!(!output.contains("password"));
        assert!(!output.contains("stream_key"));
    }
}
