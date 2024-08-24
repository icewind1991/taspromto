use crate::device::{BDAddr, RfDeviceId};
use color_eyre::{eyre::WrapErr, Report, Result};
use rumqttc::MqttOptions;
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;
use std::time::Duration;

#[derive(Default)]
pub struct Config {
    pub mqtt_host: String,
    pub mqtt_port: u16,
    pub host_port: u16,
    pub mi_temp_names: BTreeMap<BDAddr, String>,
    pub rf_temp_names: HashMap<RfDeviceId<'static>, String>,
    pub mqtt_credentials: Option<Credentials>,
}

pub struct Credentials {
    username: String,
    password: String,
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
                Some(Credentials { username, password })
            }
            Err(_) => None,
        };

        Ok(Config {
            mqtt_host,
            mqtt_port,
            host_port,
            mi_temp_names,
            rf_temp_names,
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
        mqtt_options.set_keep_alive(Duration::from_secs(5));
        Ok(mqtt_options)
    }
}
