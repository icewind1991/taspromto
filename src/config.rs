use crate::device::BDAddr;
use color_eyre::{eyre::WrapErr, Report, Result};
use rumqttc::MqttOptions;
use std::collections::BTreeMap;
use std::str::FromStr;

#[derive(Default)]
pub struct Config {
    pub mqtt_host: String,
    pub mqtt_port: u16,
    pub host_port: u16,
    pub mi_temp_names: BTreeMap<BDAddr, String>,
    pub mqtt_credentials: Option<Credentials>,
}

pub struct Credentials {
    username: String,
    password: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let mqtt_host = dotenv::var("MQTT_HOSTNAME").wrap_err("MQTT_HOSTNAME not set")?;
        let mqtt_port = dotenv::var("MQTT_PORT")
            .ok()
            .and_then(|port| u16::from_str(&port).ok())
            .unwrap_or(1883);
        let host_port = dotenv::var("PORT")
            .ok()
            .and_then(|port| u16::from_str(&port).ok())
            .unwrap_or(80);

        let mi_temp_names = dotenv::var("MITEMP_NAMES").unwrap_or_default();
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

        let mqtt_credentials = match dotenv::var("MQTT_USERNAME") {
            Ok(username) => {
                let password = dotenv::var("MQTT_PASSWORD")
                    .wrap_err("MQTT_USERNAME set, but MQTT_PASSWORD not set")?;
                Some(Credentials { username, password })
            }
            Err(_) => None,
        };

        Ok(Config {
            mqtt_host,
            mqtt_port,
            host_port,
            mi_temp_names,
            mqtt_credentials,
        })
    }

    pub fn mqtt(&self) -> Result<MqttOptions> {
        let hostname = hostname::get()?
            .into_string()
            .map_err(|_| Report::msg("invalid hostname"))?;
        let mut mqtt_options = MqttOptions::new(
            format!("taspromto-{}", hostname),
            &self.mqtt_host,
            self.mqtt_port,
        );
        if let Some(credentials) = self.mqtt_credentials.as_ref() {
            mqtt_options.set_credentials(&credentials.username, &credentials.password);
        }
        mqtt_options.set_keep_alive(5);
        Ok(mqtt_options)
    }
}
