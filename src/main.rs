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
use rumqttc::QoS;
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
    let mi_temp_names = config.mi_temp_names.clone();
    tokio::task::spawn(async move {
        loop {
            if let Err(e) = mqtt_client(&config, states.clone()).await {
                eprintln!("lost mqtt collection: {:#}", e);
            }
            eprintln!("reconnecting after 1s");
            tokio::time::delay_for(Duration::from_secs(1)).await;
        }
    });

    let state = warp::any().map(move || device_states.clone());

    let metrics = warp::path!("metrics")
        .and(state)
        .map(move |state: DeviceStates| {
            let mut response = String::new();
            for device in state.iter() {
                format_device_state(
                    &mut response,
                    &device.key(),
                    &device.value(),
                    &mi_temp_names,
                )
                .unwrap();
            }
            response
        });

    warp::serve(metrics).run(([0, 0, 0, 0], host_port)).await;
    Ok(())
}

async fn mqtt_client(config: &Config, device_states: DeviceStates) -> Result<()> {
    let mqtt_options = config.mqtt()?;

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
                let send_client = client.clone();
                tokio::task::spawn(async move {
                    if let Err(e) = send_client
                        .publish(
                            device.get_topic("cmnd", "POWER"),
                            QoS::AtMostOnce,
                            false,
                            "",
                        )
                        .await
                    {
                        eprintln!("Failed to ask for power state: {:#}", e);
                    }
                    if let Err(e) = send_client
                        .publish(
                            device.get_topic("cmnd", "DeviceName"),
                            QoS::AtMostOnce,
                            false,
                            "",
                        )
                        .await
                    {
                        eprintln!("Failed to ask for device name: {:#}", e);
                    }
                });
            }
            Topic::Power(_) => {}
            Topic::Result(device) => {
                let payload = std::str::from_utf8(message.payload.as_ref()).unwrap_or_default();
                if let Ok(json) = json::parse(payload) {
                    let mut device_state = device_states.entry(device).or_default();
                    device_state.update(json);
                }
            }
            Topic::Sensor(device) => {
                let payload = std::str::from_utf8(message.payload.as_ref()).unwrap_or_default();
                if let Ok(json) = json::parse(payload) {
                    let mut device_state = device_states.entry(device).or_default();
                    device_state.update(json);
                }
            }
            _ => {}
        }
    }
    Ok(())
}
