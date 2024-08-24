mod config;
mod device;
mod mqtt;
mod topic;

use crate::config::Config;
use crate::device::{
    format_device_state, format_dsmr_state, format_mi_temp_state, format_rf_temp_state, Device,
    DeviceStates,
};
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
    let mqtt_options = config.mqtt()?;

    let device_states = <Arc<Mutex<DeviceStates>>>::default();

    ctrlc::set_handler(move || {
        std::process::exit(0);
    })
    .expect("Error setting Ctrl-C handler");

    spawn(serve(device_states.clone(), config));

    loop {
        let (client, stream) = mqtt_stream(mqtt_options.clone())
            .await
            .wrap_err("Failed to setup mqtt listener")?;

        let cleanup_task = spawn(cleanup(client.clone(), device_states.clone()));

        pin_mut!(stream);

        if let Err(e) = mqtt_client(client.clone(), &mut stream, device_states.clone()).await {
            eprintln!("lost mqtt collection: {:#}", e);
        }
        eprintln!("reconnecting after 1s");
        sleep(Duration::from_secs(1)).await;

        cleanup_task.abort();
    }
}

async fn serve(device_states: Arc<Mutex<DeviceStates>>, config: Config) {
    let host_port = config.host_port;
    let mi_temp_names = config.mi_temp_names.clone();
    let rf_temp_names = config.rf_temp_names.clone();

    let state = warp::any().map(move || device_states.clone());

    let metrics = warp::path!("metrics")
        .and(state)
        .map(move |state: Arc<Mutex<DeviceStates>>| {
            let state = state.lock().unwrap();
            let mut response = String::new();
            for (device, state) in state.devices() {
                format_device_state(&mut response, device, state).unwrap();
            }
            for (device, state) in state.dsmr_devices() {
                format_dsmr_state(&mut response, device.hostname.as_str(), state).unwrap();
            }
            for (addr, state) in state.mi_temp() {
                format_mi_temp_state(&mut response, *addr, &mi_temp_names, state).unwrap()
            }
            for (channel, state) in state.rf_temp() {
                format_rf_temp_state(&mut response, &channel, &rf_temp_names, state).unwrap()
            }
            response
        });

    warp::serve(metrics).run(([0, 0, 0, 0], host_port)).await;
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
                if let Ok(json) = jzon::parse(payload) {
                    let mut device_states = device_states.lock().unwrap();
                    device_states.update(device, json);
                }
            }
            Topic::Msg(_device) => {
                let payload = std::str::from_utf8(message.payload.as_ref()).unwrap_or_default();
                let mut device_states = device_states.lock().unwrap();
                device_states.update_rf(payload);
            }
            Topic::Rtl(device, field) => {
                let payload = std::str::from_utf8(message.payload.as_ref()).unwrap_or_default();
                let mut device_states = device_states.lock().unwrap();
                device_states.update_rtl(&device.hostname, &field, payload);
            }
            topic @ (Topic::Water(_)
            | Topic::Gas(_)
            | Topic::Energy1(_)
            | Topic::Energy2(_)
            | Topic::DsmrPower(_)) => {
                let payload = std::str::from_utf8(message.payload.as_ref()).unwrap_or_default();
                let mut device_states = device_states.lock().unwrap();
                if let Some(ty) = topic.dsmr_type() {
                    device_states.update_dsmr(topic.into_device(), ty, payload);
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
