# bruce-embedded-std
Bruce's Standard Library Embedded Platform: the hardware agnostic part of a platform for embedded applications, which run one or more *Features*, each with its settings and a config page to change them. It includes the standard Rust library, so the heap and standard collections like ```Vec``` are all available.

The platform is split in two, by one rule: this repository holds the code which would work the same on any hardware with the standard library, and a platform repository holds the code specific to its hardware, implementing this one's traits. There is one platform so far, [bruce-esp-idf](https://github.com/bruceskingle/bruce-esp-idf), for ESP32 boards, which has the boards' demos and most of the documentation; a platform for other hardware, e.g. a Raspberry Pi running Linux, would be another beside it. This repository depends on no hardware, so it builds, and its tests run, on a computer.

| Crate | What it is |
|---|---|
| [bruce-embedded-std](bruce-embedded-std) | *Features* and their configuration, the config page, the task scheduler, time zones, graphics such as the clocks and the colours, the mDNS responder, the attitude and tilt worked out from an IMU's readings (the ```ahrs``` and ```tilt``` features), and the traits a platform implements, e.g. ```Platform```, ```ConfigStore```, ```HttpServerManager``` and ```DisplayManager```. |
| [bruce-embedded-std-derive](bruce-embedded-std-derive) | ```#[derive(FeatureConfig)]``` for a *Feature*'s settings, which bruce-embedded-std re-exports. |

The crates are released together, with the same version.

# The Core Platform
The platform provides a set of services to support the running of one or more *Features*, which provide the application functionality. Each feature can be enabled or disabled and may have a configuration. The *Core Feature* provides WiFi and an HTTP server. If the *Core Feature* does not have a valid configuration (including the SSID and password for your WiFi network), then it will start WiFi an *Access Point Mode*. This means that it will create a new WiFi network (with the SSID **ESP32-Setup** and the password **password**) with a captive portal so that if you connect to this network you will be taken automatically to the config page so the necessary configuration values can be entered. Once this is done and the board is rebooted it will start WiFi in client mode, connecting to the WiFi network you have configured as a normal device.

The HTTP server is still available (on the IP address allocated by your WiFi network) which means that further configuration is possible and this HTTP server can also be used by *Features* to provide application functionality. The *Core Feature* includes an mDNS server so that you can easily find the board using a DNS name in the **.local** domain, and a task scheduler which can be used to run task processes on a predetermined schedule using [Cron](https://en.wikipedia.org/wiki/Cron) like schedules, and an [NTP](https://en.wikipedia.org/wiki/Ntpd) client which synchronies the system clock with real time.

# Settings
A *Feature*'s settings are a struct with ```#[derive(FeatureConfig)]```, which describes them to the config page: ```#[config(len = N)]``` on a string, ```#[config(secret)]``` on a password, ```#[config(label = "...")]``` for what the page shows, see the derive's documentation.

## Sliders
A number setting can be shown on the config page as a slider, with ```#[config(min = 0, max = 100)]``` on an ```i32``` or ```i64``` field of a *Feature*'s config. ```default = N``` is where the slider is while the setting has not been saved, and ```preview = "/path"``` a path on the device which the page POSTs each position to, as ```value=N```, while the slider is moved, so that its effect can be tried before it is saved, as the speaker's volume is. A position is sent only once the last has been answered, so that a fast drag does not queue up requests.

# Design Patterns
The platform makes use of the builder pattern as a way of trying to get the best trade off between the cost and benefits of the standard library. Most of the platform types have a builder containing collections which allow items to be added, but the ```build()``` method will usually call ```.shrink_to_fit()``` on those collections which are then treated as immutable from that point onwards.

The conventions for builders are that they
- are constructed by calling the ```builder()``` associated function on the class being constructed, e.g. ```TaskManager::builder()```
- provide chainable methods with names starting ```with_``` e.g. 
```
    TaskManager::builder()
        .with_task(task1)?
        .with_task(task2)?
        .build();
```
- provide callable methods with names starting ```add_``` e.g.
```
    let mut builder = TaskManager::builder()
        .with_task(task1)?;
    
    builder.add_task(task2)?;
    
    builder.build();
```
- have methods which either return their result directly (if there are no failure scenarios) or return an ```anyhow::Result<>``` of their result.

# Building and Testing
```sh
cargo test --workspace --all-features
```

License
=======

Licensed under either of

* Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

Contribution
------------

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
