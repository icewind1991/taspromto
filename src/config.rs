use crate::device::{BDAddr, RfDeviceId};
use color_eyre::{eyre::WrapErr, Report, Result};
use rumqttc::MqttOptions;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::fs::read_to_string;
use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub listen: ListenConfig,
    pub names: NamesConfig,
    pub mqtt: MqttConfig,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ListenConfig {
    Ip {
        #[serde(default = "default_address")]
        address: IpAddr,
        port: u16,
    },
    Unix {
        path: String,
    },
}

fn default_address() -> IpAddr {
    Ipv4Addr::UNSPECIFIED.into()
}

#[derive(Debug, Deserialize)]
pub struct NamesConfig {
    #[serde(rename = "mitemp")]
    pub mi_temp: BTreeMap<BDAddr, String>,
    #[serde(rename = "rftemp")]
    pub rf_temp: HashMap<RfDeviceId<'static>, String>,
}

#[derive(Debug, Deserialize)]
pub struct MqttConfig {
    #[serde(rename = "hostname")]
    host: String,
    #[serde(default = "default_mqtt_port")]
    port: u16,
    #[serde(flatten)]
    credentials: Option<Credentials>,
}

fn default_mqtt_port() -> u16 {
    1883
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Credentials {
    Raw {
        username: String,
        password: String,
    },
    File {
        username: String,
        password_file: String,
    },
}

impl Credentials {
    pub fn username(&self) -> String {
        match self {
            Credentials::Raw { username, .. } => username.clone(),
            Credentials::File { username, .. } => username.clone(),
        }
    }
    pub fn password(&self) -> String {
        match self {
            Credentials::Raw { password, .. } => password.clone(),
            Credentials::File { password_file, .. } => secretfile::load(password_file).unwrap(),
        }
    }
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let mqtt_host = dotenvy::var("MQTT_HOSTNAME").wrap_err("MQTT_HOSTNAME not set")?;
        let mqtt_port = dotenvy::var("MQTT_PORT")
            .ok()
            .and_then(|port| u16::from_str(&port).ok())
            .unwrap_or(1883);
        let host_port = dotenvy::var("PORT")
            .ok()
            .and_then(|port| u16::from_str(&port).ok())
            .unwrap_or(80);

        let mi_temp_names = dotenvy::var("MITEMP_NAMES").unwrap_or_default();
        let mi_temp_names = mi_temp_names
            .split(',')
            .map(|pair| {
                let mut parts = pair.split('=');
                if let (Some(mac), Some(name)) = (
                    parts.next().map(BDAddr::from_mi_temp_mac_part),
                    parts.next(),
                ) {
                    let mac = mac.wrap_err("Invalid MITEMP_NAMES")?;
                    Ok((mac, name.to_string()))
                } else {
                    Err(Report::msg("Invalid MITEMP_NAMES"))
                }
            })
            .collect::<Result<BTreeMap<BDAddr, String>, Report>>()?;

        let rf_temp_names = dotenvy::var("RF_TEMP_NAMES").unwrap_or_default();
        let rf_temp_names = rf_temp_names
            .split(',')
            .map(|pair| {
                let mut parts = pair.split('=');
                if let (Some(channel), Some(name)) = (parts.next(), parts.next()) {
                    let device_id =
                        RfDeviceId::from_str(channel).wrap_err("Invalid RF_TEMP_NAMES")?;
                    Ok((device_id, name.to_string()))
                } else {
                    Err(Report::msg("Invalid RF_TEMP_NAMES"))
                }
            })
            .collect::<Result<HashMap<_, _>, Report>>()?;

        let mqtt_credentials = match dotenvy::var("MQTT_USERNAME") {
            Ok(username) => {
                let password = dotenvy::var("MQTT_PASSWORD")
                    .wrap_err("MQTT_USERNAME set, but MQTT_PASSWORD not set")?;
                Some(Credentials::Raw { username, password })
            }
            Err(_) => None,
        };

        Ok(Config {
            listen: ListenConfig::Ip {
                port: host_port,
                address: default_address(),
            },
            names: NamesConfig {
                mi_temp: mi_temp_names,
                rf_temp: rf_temp_names,
            },
            mqtt: MqttConfig {
                port: mqtt_port,
                host: mqtt_host,
                credentials: mqtt_credentials,
            },
        })
    }

    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Config> {
        let raw = read_to_string(path)?;
        Ok(toml::from_str(&raw)?)
    }

    pub fn mqtt(&self) -> Result<MqttOptions> {
        let hostname = hostname::get()?
            .into_string()
            .map_err(|_| Report::msg("invalid hostname"))?;
        let mut mqtt_options = MqttOptions::new(
            format!("taspromto-{}", hostname),
            &self.mqtt.host,
            self.mqtt.port,
        );
        if let Some(credentials) = self.mqtt.credentials.as_ref() {
            mqtt_options.set_credentials(credentials.username(), credentials.password());
        }
        mqtt_options.set_keep_alive(Duration::from_secs(5));
        Ok(mqtt_options)
    }
}
