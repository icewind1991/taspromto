use dashmap::DashMap;
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use std::convert::TryFrom;
use std::fmt::Write;
use std::str::FromStr;
use std::sync::Arc;
use tokio::time::Duration;
use warp::Filter;

type DeviceStates = Arc<DashMap<Device, DeviceState>>;

#[tokio::main]
async fn main() {
    let mqtt_host = dotenv::var("MQTT_HOSTNAME").expect("MQTT_HOSTNAME not set");
    let mqtt_port = dotenv::var("MQTT_PORT")
        .ok()
        .and_then(|port| u16::from_str(&port).ok())
        .unwrap_or(1883);
    let host_port = dotenv::var("PORT")
        .ok()
        .and_then(|port| u16::from_str(&port).ok())
        .unwrap_or(80);

    let device_states = DeviceStates::default();

    ctrlc::set_handler(move || {
        std::process::exit(0);
    })
    .expect("Error setting Ctrl-C handler");

    let states = device_states.clone();
    tokio::task::spawn(async move {
        loop {
            mqtt_client(&mqtt_host, mqtt_port, states.clone()).await;
            eprintln!("lost mqtt collection, reconnecting after 1s");
            tokio::time::delay_for(Duration::from_secs(1)).await;
        }
    });

    let state = warp::any().map(move || device_states.clone());
    let metrics = warp::path!("metrics")
        .and(state)
        .map(|state: DeviceStates| {
            let mut response = String::new();
            for device in state.iter() {
                writeln!(
                    &mut response,
                    "switch_state{{tasmota_id=\"{}\", name=\"{}\"}} {}",
                    device.key().hostname,
                    device.name,
                    if device.state { 1 } else { 0 }
                )
                .unwrap();

                if let Some(power_watts) = device.power_watts {
                    writeln!(
                        &mut response,
                        "power_watts{{tasmota_id=\"{}\", name=\"{}\"}} {}",
                        device.key().hostname,
                        device.name,
                        power_watts
                    )
                    .unwrap();
                }

                if let Some(power_yesterday) = device.power_yesterday {
                    writeln!(
                        &mut response,
                        "power_yesterday_kwh{{tasmota_id=\"{}\", name=\"{}\"}} {}",
                        device.key().hostname,
                        device.name,
                        power_yesterday
                    )
                    .unwrap();
                }

                if let Some(power_today) = device.power_today {
                    writeln!(
                        &mut response,
                        "power_today_kwh{{tasmota_id=\"{}\", name=\"{}\"}} {}",
                        device.key().hostname,
                        device.name,
                        power_today
                    )
                    .unwrap();
                }
            }
            response
        });

    warp::serve(metrics).run(([0, 0, 0, 0], host_port)).await;
}

async fn mqtt_client(host: &str, port: u16, device_states: DeviceStates) {
    let mut mqttoptions = MqttOptions::new("rumqtt-async", host, port);
    mqttoptions.set_keep_alive(5);

    let (client, mut event_loop) = AsyncClient::new(mqttoptions, 10);
    client
        .subscribe("tele/+/+/LWT", QoS::AtMostOnce)
        .await
        .unwrap();
    client
        .subscribe("stat/+/+/POWER", QoS::AtMostOnce)
        .await
        .unwrap();
    client
        .subscribe("tele/+/+/SENSOR", QoS::AtMostOnce)
        .await
        .unwrap();
    client
        .subscribe("stat/+/+/RESULT", QoS::AtMostOnce)
        .await
        .unwrap();

    while let Ok(notification) = event_loop.poll().await {
        if let Event::Incoming(Packet::Publish(message)) = notification {
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
                        .await
                        .unwrap();
                    client
                        .publish(
                            device.get_topic("cmnd", "DeviceName"),
                            QoS::AtMostOnce,
                            false,
                            "",
                        )
                        .await
                        .unwrap();
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
                            device_state.name = json["DeviceName"].to_string();
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
    }
}

#[derive(Debug, Eq, PartialEq, Clone, Hash)]
struct Device {
    topic: String,
    hostname: String,
}

impl Device {
    fn get_topic(&self, prefix: &str, command: &str) -> String {
        format!("{}/{}/{}/{}", prefix, self.topic, self.hostname, command)
    }
}

#[derive(Debug, Default)]
struct DeviceState {
    state: bool,
    name: String,
    power_watts: Option<f32>,
    power_yesterday: Option<f32>,
    power_today: Option<f32>,
}

#[derive(Debug, Eq, PartialEq)]
enum Topic {
    LWT(Device),
    Power(Device),
    State(Device),
    Sensor(Device),
    Result(Device),
    Other(String),
}

impl From<&str> for Topic {
    fn from(raw: &str) -> Self {
        let mut parts = raw.split('/');
        if let (Some(prefix), Some(topic), Some(hostname), Some(cmd)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        {
            let device = Device {
                topic: topic.to_string(),
                hostname: hostname.to_string(),
            };
            match (prefix, cmd) {
                ("tele", "LWT") => Topic::LWT(device),
                ("tele", "STATE") => Topic::State(device),
                ("stat", "POWER") => Topic::Power(device),
                ("tele", "SENSOR") => Topic::Sensor(device),
                ("stat", "RESULT") => Topic::Result(device),
                _ => Topic::Other(raw.to_string()),
            }
        } else {
            Topic::Other(raw.to_string())
        }
    }
}

#[test]
fn parse_topic() {
    let device = Device {
        hostname: "hostname".to_string(),
        topic: "foo".to_string(),
    };
    assert_eq!(
        Topic::LWT(device.clone()),
        Topic::from("tele/foo/hostname/LWT")
    );
    assert_eq!(
        Topic::Power(device.clone()),
        Topic::from("stat/foo/hostname/POWER")
    );
    assert_eq!(
        Topic::State(device.clone()),
        Topic::from("tele/foo/hostname/STATE")
    );
    assert_eq!(
        Topic::Sensor(device.clone()),
        Topic::from("tele/foo/hostname/SENSOR")
    );
    assert_eq!(
        Topic::Result(device.clone()),
        Topic::from("stat/foo/hostname/RESULT")
    );
}
