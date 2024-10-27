# taspromto

Publish tasmota (and other) state into prometheus

## What

Taspromto listens to messages published by tasmota (and other) devices to MQTT and presents the data in a prometheus
compatible
format.

## Usage

Run the binary with the following environment variables set

```dotenv
PORT=
MQTT_HOSTNAME=
MQTT_USERNAME= # Optional
MQTT_PASSWORD= # Optional
```

## Exposed data

The following tasmota data is supported

- ON/OFF state
- Current and total power consumption for power meter devices
- CO² levels for [MH-Z19 sensors](https://tasmota.github.io/docs/MH-Z19B/)
- Power and Gas levels from [supported P1 smart meters](https://tasmota.github.io/docs/Smart-Meter-Interface/)
- Particle concentration from PMS5003 sensors
- 433Mhz temperature sensor readings from [`rtl_433`](https://github.com/merbanan/rtl_433)

## Xiaomi MI Temperature and Humidity Sensors

Tasmota can expose temperature and humidity data from Xiaomi sensors, to expose these sensors you need to configure the
names for the sensors.

This is done by setting the `MITEMP_NAMES` environment variable to comma separated key value pairs of the last 6 digits
of the MAC address of the sensors and the desired name.

```dotenv
MITEMP_NAMES="351234=Bedroom,352468=Living Room"
```

## 433Mhz temperature sensors

Taspromto can parse data 433Mhz temperature sensors send to MQTT by [`rtl_433`](https://github.com/merbanan/rtl_433).

rtl_433 needs to be configured to send it's output to the `rtl_433[/model]` topic, then sensors can be configured by
setting
the `RF_TEMP_NAMES` environment variable to comma seperated key value pairs. Where the key is the sensor model, id and
channel.

```dotenv
RF_TEMP_NAMES="Bresser-3CH:73:1=Front Yard,Bresser-3CH:73:2=Attic"
```
