mod config;
mod device;
mod mqtt;
mod topic;

use crate::config::Config;
use crate::device::{format_device_state, format_mi_temp_state, Device, DeviceStates};
use crate::mqtt::mqtt_stream;
use crate::topic::Topic;
use color_eyre::{eyre::WrapErr, Result};

use pin_utils::pin_mut;
use rumqttc::{AsyncClient, Publish, QoS};

use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::task::spawn;
use tokio::time::{sleep, Duration};
use tokio_stream::{Stream, StreamExt};
use warp::Filter;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::from_env()?;
    let host_port = config.host_port;

    let device_states = <Arc<Mutex<DeviceStates>>>::default();

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
        .map(move |state: Arc<Mutex<DeviceStates>>| {
            let state = state.lock().unwrap();
            let mut response = String::new();
            for (device, state) in state.devices() {
                format_device_state(&mut response, device, state).unwrap();
            }
            for (addr, state) in state.mi_temp() {
                format_mi_temp_state(&mut response, *addr, &mi_temp_names, state).unwrap()
            }
            response
        });

    warp::serve(metrics).run(([0, 0, 0, 0], host_port)).await;
    Ok(())
}

async fn mqtt_loop(
    client: AsyncClient,
    stream: impl Stream<Item = Result<Publish>>,
    states: Arc<Mutex<DeviceStates>>,
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

async fn command(client: &AsyncClient, device: &Device, command: &str, body: &str) -> Result<()> {
    client
        .publish(
            device.get_topic("cmnd", command),
            QoS::AtMostOnce,
            false,
            body,
        )
        .await?;
    Ok(())
}

async fn mqtt_client<S: Stream<Item = Result<Publish>>>(
    client: AsyncClient,
    stream: &mut Pin<&mut S>,
    device_states: Arc<Mutex<DeviceStates>>,
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
            Topic::Lwt(device) => {
                // on discovery, ask the device for it's power state and name
                let send_client = client.clone();
                spawn(async move {
                    if let Err(e) = command(&send_client, &device, "POWER", "").await {
                        eprintln!("Failed to ask for power state: {:#}", e);
                    }
                    if let Err(e) = command(&send_client, &device, "DeviceName", "").await {
                        eprintln!("Failed to ask for device name: {:#}", e);
                    }
                    if let Err(e) = command(&send_client, &device, "Status", "2").await {
                        eprintln!("Failed to ask for firmware state: {:#}", e);
                    }
                });
            }
            Topic::Power(_) => {}
            Topic::Result(device) | Topic::Sensor(device) | Topic::Status(device) => {
                let payload = std::str::from_utf8(message.payload.as_ref()).unwrap_or_default();
                if let Ok(json) = json::parse(payload) {
                    let mut device_states = device_states.lock().unwrap();
                    device_states.update(device, json);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

async fn cleanup(client: AsyncClient, state: Arc<Mutex<DeviceStates>>) {
    loop {
        let ping_time = Instant::now() - Duration::from_secs(10 * 60);
        let cleanup_time = Instant::now() - Duration::from_secs(15 * 60);

        state
            .lock()
            .unwrap()
            .retain(cleanup_time, ping_time, &client);

        sleep(Duration::from_secs(60)).await;
    }
}
