mod config;
mod device;
mod mqtt;
mod topic;

use crate::config::Config;
use crate::device::{format_device_state, Device, DeviceState};
use crate::mqtt::mqtt_stream;
use crate::topic::Topic;
use color_eyre::{eyre::WrapErr, Result};
use dashmap::DashMap;
use pin_utils::pin_mut;
use rumqttc::{MqttOptions, QoS};
use std::convert::TryFrom;
use std::sync::Arc;
use tokio::stream::StreamExt;
use tokio::time::Duration;
use warp::Filter;

type DeviceStates = Arc<DashMap<Device, DeviceState>>;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::from_env()?;
    let host_port = config.host_port;

    let device_states = DeviceStates::default();

    ctrlc::set_handler(move || {
        std::process::exit(0);
    })
    .expect("Error setting Ctrl-C handler");

    let states = device_states.clone();
    tokio::task::spawn(async move {
        loop {
            if let Err(e) = mqtt_client(&config.mqtt_host, config.mqtt_port, states.clone()).await {
                eprintln!("lost mqtt collection: {:#}", e);
            }
            eprintln!("reconnecting after 1s");
            tokio::time::delay_for(Duration::from_secs(1)).await;
        }
    });

    let state = warp::any().map(move || device_states.clone());
    let metrics = warp::path!("metrics")
        .and(state)
        .map(|state: DeviceStates| {
            let mut response = String::new();
            for device in state.iter() {
                format_device_state(&mut response, &device.key(), &device.value()).unwrap();
            }
            response
        });

    warp::serve(metrics).run(([0, 0, 0, 0], host_port)).await;
    Ok(())
}

async fn mqtt_client(host: &str, port: u16, device_states: DeviceStates) -> Result<()> {
    let mut mqtt_options = MqttOptions::new("taspromto", host, port);
    mqtt_options.set_keep_alive(5);

    let (client, stream) = mqtt_stream(mqtt_options)
        .await
        .wrap_err("Failed to setup mqtt listener")?;

    pin_mut!(stream);

    while let Some(message) = stream.next().await {
        let message = message?;
        println!(
            "{} {}",
            message.topic,
            std::str::from_utf8(message.payload.as_ref()).unwrap_or_default()
        );
        let topic = Topic::from(message.topic.as_str());

        match topic {
            Topic::LWT(device) => {
                // on discovery, ask the device for it's power state and name
                client
                    .publish(
                        device.get_topic("cmnd", "POWER"),
                        QoS::AtMostOnce,
                        false,
                        "",
                    )
                    .await?;
                client
                    .publish(
                        device.get_topic("cmnd", "DeviceName"),
                        QoS::AtMostOnce,
                        false,
                        "",
                    )
                    .await?;
            }
            Topic::Power(device) => {
                let state = message.payload.as_ref() == b"ON";
                device_states.entry(device).or_default().state = state;
            }
            Topic::Result(device) => {
                let payload = std::str::from_utf8(message.payload.as_ref()).unwrap_or_default();
                if let Ok(json) = json::parse(payload) {
                    let mut device_state = device_states.entry(device).or_default();
                    if json["DeviceName"].is_string() {
                        let name = json["DeviceName"].to_string();
                        if !name.is_empty() {
                            device_state.name = name;
                        }
                    }
                }
            }
            Topic::Sensor(device) => {
                let payload = std::str::from_utf8(message.payload.as_ref()).unwrap_or_default();
                if let Ok(json) = json::parse(payload) {
                    let mut device_state = device_states.entry(device).or_default();
                    device_state.power_watts = json["ENERGY"]["Power"]
                        .as_number()
                        .map(|num| f32::try_from(num).unwrap_or_default());
                    device_state.power_yesterday = json["ENERGY"]["Yesterday"]
                        .as_number()
                        .map(|num| f32::try_from(num).unwrap_or_default());
                    device_state.power_today = json["ENERGY"]["Today"]
                        .as_number()
                        .map(|num| f32::try_from(num).unwrap_or_default());
                }
            }
            _ => {}
        }
    }
    Ok(())
}
