# Meshstellar ✨ - Monitor your Meshtastic constellation

Welcome to Meshstellar, an open-source tool designed for monitoring and analyzing your local Meshtastic network. Meshtastic is a powerful platform that uses long-range, low-power radio mesh networking, ideal for communication beyond the reach of traditional connectivity. Meshstellar aims to complement this by offering insights into network traffic and node health, helping you keep your network running smoothly.

![Screenshot of the packets list of meshstellar](screenshot.png?raw=true)

## Features

- Traffic Monitoring: Track the flow of packets within your network, including their origin, destination, and payload, to better understand network activity.
- Health Analysis: View critical metrics such as battery levels, signal strength, and channel utilization for each node to quickly spot and resolve potential issues.
- Node Overview: Gain a quick summary of all nodes in your network, including their status and location, to maintain a clear view of your network's layout.
- Neighbor Insights: Understand how nodes are interconnected by using Neighborinfo packets.
- Device Metrics: Access important device performance indicators, including voltage and airtime utilization, to make informed decisions about node management.
- Off-grid support: The application can run fully local (no external resources).

## Technical overview

Meshstellar is a straightforward project aimed at Meshtastic network users. It's written in Rust and stores data in a SQLite database.
Its frontend tries to use as little JavaScript as possible, and uses HTMX and _hyperscript.

### How it works

Meshstellar listens for data from Meshtastic devices sent over MQTT, like messages or node stats. This data, packed in Protocol Buffers, is then decoded and stored in a SQLite database (which by design stays as close to the source data as possible). 
It's a simple approach to log and keep track of what's happening in your network.

## Configuration

There are a few configuration file examples in this repository. 

On Windows you can configure it using the `meshstellar.toml` file (rename it from `meshstellar.toml.example` and put it in the same directory as the .exe file).

On Linux / Docker it's probably easiest to configure it using environment variables (see `.env.example` for a list of environment variables you can configure). 

## How to run

### Windows

Make sure the configuration (the `meshtastic.toml` file) is updated and saved in the same directory as the meshstellar.exe file. Double click the .exe to start meshstellar.

### Use docker-compose

See `docker-compose.yml.example` for an example deployment.

### Manually on command line

Meshstellar can be run in different modes based on the command-line argument provided when starting the application.

The simplest mode is the `all` mode, which starts the MQTT processing and web server in a single process:

```sh
meshstellar [all]
```

A more robust setup has separate processes for the mqtt ingestion, packet importing and web service functionality.

```sh
meshstellar mqtt
meshstellar import
meshstellar web
```

## Contributing

Your contributions and feedback are welcome!
