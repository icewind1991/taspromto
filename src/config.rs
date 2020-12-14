use crate::device::BDAddr;
use color_eyre::{eyre::WrapErr, Report, Result};
use std::collections::BTreeMap;
use std::str::FromStr;

#[derive(Default)]
pub struct Config {
    pub mqtt_host: String,
    pub mqtt_port: u16,
    pub host_port: u16,
    pub mi_temp_names: BTreeMap<BDAddr, String>,
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

        Ok(Config {
            mqtt_host,
            mqtt_port,
            host_port,
            mi_temp_names,
        })
    }
}
