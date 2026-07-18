# LIVEU STATS BOT

A community-maintained chat bot for monitoring and controlling a regular LiveU Solo unit from Twitch chat.

This project uses the same private web services as the regular
[LiveU Solo portal](https://solo.liveu.tv/). It does not require the separate LiveU Central
product. These services are not a public, supported Solo API and may change without notice.

## How do i run this?

Just download the latest binary from [releases](https://github.com/BigChiefRick/liveu_stats_bot/releases) and execute it.

### Docker

The image intentionally does not contain `config.json`. Mount your local configuration read-only
when starting it:

```sh
docker run --rm \
  -v /path/to/config.json:/app/config.json:ro \
  rickmns/liveu_stats_bot:pr-2
```

Keeping the configuration outside the image prevents LiveU and Twitch credentials from being
published in an image layer.

## Config

This config will be automatically generated upon running the binary and saved as `config.json`.

```JSON
{
    "liveu": {
        "email": "YOUR LIVEU EMAIL",
        "password": "YOUR LIVEU PASSWORD",
        "id": null,
        "monitor": {
            "battery": true,
            "batteryNotification": [
                99,
                50,
                10,
                5,
                1
            ],
            "modems": true
        }
    },
    "twitch": {
        "botUsername": "TWITCH BOT USERNAME",
        "botOauth": "TWITCH BOT OAUTH",
        "channel": "YOUR TWITCH CHANNEL",
        "adminUsers": ["b3ck", "travelwithgus"],
        "modOnly": true
    },
    "commands": {
        "cooldown": 5,
        "stats": ["!lustats", "!liveustats", "!lus"],
        "battery": ["!battery", "!liveubattery", "!lub"],
        "destinations": "!ludestinations",
        "firmware": "!lufirmware",
        "interfaces": "!lucarrier",
        "streamtools": "!lustreamtools",
        "config": "!luconfig",
        "start": "!lustart",
        "stop": "!lustop",
        "restart": "!lurestart",
        "reboot": "!lureboot",
        "delay": "!ludelay"
    },
    "rtmp": {
        "url": "http://localhost/stat",
        "application": "publish",
        "key": "live"
    },
    "customPortNames": {
        "ethernet": "ETH",
        "wifi": "WiFi",
        "usb1": "USB1",
        "usb2": "USB2",
        "sim1": "SIM1",
        "sim2": "SIM2"
    }
}
```

### Optional config settings

You can remove these settings from the config if you don't want them or replace them with `null`.

| Name            | Description                                                                         |
| --------------- | ----------------------------------------------------------------------------------- |
| id              | When using mutliple units you can set a default unit by using the bossid            |
| adminUsers      | A list of twitch usernames e.g. `["715209", "b3ck"]`                                |
| rtmp            | If you are using nginx you can also show the bitrate when using the `stats` command |
| customPortNames | Customize the port names                                                            |

## Chat Commands

After running the app successfully you can use the following default commands in your chat:

| Name         | Default command   | Description                                                   |
| ------------ | ----------------- | ------------------------------------------------------------- |
| stats        | !lus              | Shows the current connected modems and bitrate                |
| battery      | !lub              | Shows the current battery charge percentage                   |
| destinations | !ludestinations   | Lists configured destinations and marks the selected one      |
| firmware     | !lufirmware       | Shows the installed and selectable firmware versions          |
| carrier      | !lucarrier        | Shows carrier names, link rates, and connection technology     |
| streamtools  | !lustreamtools    | Shows the safe Stream Tools status when enabled                |
| config       | !luconfig         | Shows a credential-free summary of the active unit config      |
| start        | !lustart          | Starts the stream (not the unit)                               |
| stop         | !lustop           | Stops the stream                                               |
| restart      | !lurestart        | Restarts the stream                                            |
| reboot       | !lureboot         | Reboots the unit                                               |
| delay        | !ludelay          | Toggles between low delay and high resiliency mode             |

You can add, delete or change the commands to whatever you want in `config.json` under the `commands` section.

The start, stop, restart, reboot, and delay commands are only available to the channel owner or
`adminUsers`.

The status commands never print Wi-Fi passwords, APNs, destination credentials, stream keys,
SRT passphrases, or LiveU access tokens. Stream Tools output is available only when that service
is enabled for the selected Solo account and unit.

## Give specific users access to all commands

Add the twitch username in adminUsers like this: `["715209", "b3ck"]`.

## Possible Chat Command Results:

If your LiveU is offline you'll see this in chat:
> ChatBot: LiveU Offline :(  

If your LiveU is online and ready you'll see this in chat:
> ChatBot: LiveU Online and Ready  

If your LiveU is online, streaming but not using NGINX you'll see this in chat:
> ChatBot: WiFi: 2453 Kbps, USB1: 2548 Kbps, USB2: 2328 Kbps, Ethernet: 2285 Kbps, Total LRT: 7000 Kbps

If your LiveU is online, streaming and you're using NGINX you'll see this in chat:
> ChatBot: WiFi: 2453 Kbps, USB1: 2548 Kbps, USB2: 2328 Kbps, Ethernet: 2285 Kbps, Total LRT: 7000 Kbps, RTMP: 6000 Kbps

`Please note: if one of your connections is offline it will NOT show up at all in the stats.`

## Credits:
[NOALBS/liveu_stats_bot](https://github.com/NOALBS/liveu_stats_bot): Original project and foundation for this maintained version.

[BigChiefRick](https://github.com/BigChiefRick): Current maintenance and continued development.

[Cinnabarcorp (travelingwithgus)](https://twitch.tv/travelwithgus): Initial Idea, Feedback, Use Case, and Q&A Testing.

[B3ck](https://twitch.tv/b3ck): Feedback and Q&A Testing.
