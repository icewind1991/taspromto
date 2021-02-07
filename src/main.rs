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
use rumqttc::{AsyncClient, Publish, QoS};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;
use tokio::task::spawn;
use tokio::time::{sleep, Duration};
use tokio_stream::{Stream, StreamExt};
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

    let mi_temp_names = config.mi_temp_names.clone();

    let mqtt_options = config.mqtt()?;

    let (client, stream) = mqtt_stream(mqtt_options)
        .await
        .wrap_err("Failed to setup mqtt listener")?;

    spawn(mqtt_loop(client.clone(), stream, device_states.clone()));
    spawn(cleanup(client.clone(), device_states.clone()));

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

async fn mqtt_loop(
    client: AsyncClient,
    stream: impl Stream<Item = Result<Publish>>,
    states: DeviceStates,
) {
    pin_mut!(stream);
    loop {
        if let Err(e) = mqtt_client(client.clone(), &mut stream, states.clone()).await {
            eprintln!("lost mqtt collection: {:#}", e);
        }
        eprintln!("reconnecting after 1s");
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn command(client: &AsyncClient, device: &Device, command: &str) -> Result<()> {
    client
        .publish(
            device.get_topic("cmnd", command),
            QoS::AtMostOnce,
            false,
            "",
        )
        .await?;
    Ok(())
}

async fn mqtt_client<S: Stream<Item = Result<Publish>>>(
    client: AsyncClient,
    stream: &mut Pin<&mut S>,
    device_states: DeviceStates,
) -> Result<()> {
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
                spawn(async move {
                    if let Err(e) = command(&send_client, &device, "POWER").await {
                        eprintln!("Failed to ask for power state: {:#}", e);
                    }
                    if let Err(e) = command(&send_client, &device, "DeviceName").await {
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

async fn cleanup(client: AsyncClient, devices: DeviceStates) {
    loop {
        let ping_time = Instant::now() - Duration::from_secs(10 * 60);
        let cleanup_time = Instant::now() - Duration::from_secs(15 * 60);

        devices.retain(|device, state| {
            if state.last_seen < cleanup_time {
                println!("{} hasn't been seen for 15m, removing", device.hostname);
                false
            } else if state.last_seen < ping_time || state.name.is_empty() {
                println!(
                    "{} hasn't been seen for 10m or has no name set, pinging",
                    device.hostname
                );
                let send_client = client.clone();
                let topic = device.get_topic("cmnd", "DeviceName");
                spawn(async move {
                    if let Err(e) = send_client.publish(topic, QoS::AtMostOnce, false, "").await {
                        eprintln!("Failed to ping device: {:#}", e);
                    }
                });
                true
            } else {
                true
            }
        });

        sleep(Duration::from_secs(60)).await;
    }
}
