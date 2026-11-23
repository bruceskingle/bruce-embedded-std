//! The device's mDNS responder: it answers for its name on the network, `<hostname>.local`, and advertises the DNS-SD
//! services it is given, see MdnsService, so that apps, e.g. Bruce's Smart Home, and the flasher, find the
//! device, and what it is, without being given its address.

use anyhow::Result;
use log::{error, info};
use simple_dns::{
    CLASS, Name, Packet, QTYPE, ResourceRecord, TYPE,
    rdata::{A, PTR, RData, SRV, TXT},
};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use std::{
    net::{Ipv4Addr, SocketAddr, UdpSocket},
    sync::mpsc::TryRecvError,
};

/// The DNS-SD service type every board of the platform advertises, see MdnsService, whatever its firmware, which its
/// TXT record's `fw` says, e.g. `bruce-smart-home-capture`.
pub const SERVICE_TYPE: &str = "_bruce-device._tcp";

/// The common type a board advertises as well, with the same facts, as it is a web server, which routers which pass
/// mDNS between networks pass, while they may not pass a type they do not know, e.g. SERVICE_TYPE, as a UniFi router
/// did not until told to, on 23 November 2026.
pub const HTTP_SERVICE_TYPE: &str = "_http._tcp";

/// The name DNS-SD browsers ask for the types of services on the network.
const SERVICES: &str = "_services._dns-sd._udp.local";

/// How long the records may be kept by those who hear them, in seconds: two minutes for the address, as it may change,
/// and the usual 75 minutes for the service's other records, which are announced again when they change.
const ADDRESS_TTL: u32 = 120;
const SERVICE_TTL: u32 = 4500;

/// The responder thread's stack, on a device: 6 KB was enough for a reply of the host's address alone, but a reply
/// with the services' records overflowed it, on 23 November 2026.
const MDNS_STACK: usize = 12 * 1024;

/// How often the records are announced unasked, so that caches keep them.
const ANNOUNCE_EVERY: Duration = Duration::from_secs(300);

/// The facts of a device's DNS-SD service's TXT record, `key=value`, which the platform and the firmware set and change
/// while it runs, e.g. whether it is adopted, each change announced at once, see MdnsResponder.
#[derive(Clone, Default)]
pub struct MdnsTxt {
    inner: Arc<Mutex<TxtEntries>>,
}

/// The entries of a TXT record, in the order first set, and how many times they have changed.
type TxtEntries = (Vec<(String, String)>, u32);

impl MdnsTxt {
    /// Sets key to value, announcing it if it changed.
    pub fn set(&self, key: &str, value: impl Into<String>) {
        let value = value.into();
        let mut inner = self.inner.lock().unwrap();
        match inner.0.iter_mut().find(|(known, _)| known == key) {
            Some((_, known)) if *known == value => return,
            Some((_, known)) => *known = value,
            None => inner.0.push((key.to_string(), value)),
        }
        inner.1 += 1;
    }

    /// The entries and how many times they have changed.
    fn snapshot(&self) -> TxtEntries {
        self.inner.lock().unwrap().clone()
    }
}

/// A DNS-SD service which a device advertises: its type, e.g. SERVICE_TYPE, its instance's name, unique on the network,
/// e.g. the device's ID, its port, and its TXT record's facts.
#[derive(Clone)]
pub struct MdnsService {
    pub service_type: String,
    pub instance: String,
    pub port: u16,
    pub txt: MdnsTxt,
}

impl MdnsService {
    /// `<type>.local`, which a browser asks for, e.g. `_bruce-device._tcp.local`.
    fn type_name(&self) -> String {
        format!("{}.local", self.service_type)
    }

    /// `<instance>.<type>.local`, the name of the service's SRV and TXT records.
    fn instance_name(&self) -> String {
        format!("{}.{}.local", self.instance, self.service_type)
    }
}

pub struct MdnsResponder {
    wifi_receiver: Option<Receiver<Ipv4Addr>>,
}

/// What is answered: the host's name and address, and the services.
struct Records<'s> {
    hostname: &'s str,
    ip: Ipv4Addr,
    services: &'s [MdnsService],
}

impl Records<'_> {
    /// The host's address record.
    fn address(&self) -> ResourceRecord<'static> {
        ResourceRecord::new(
            Name::new_unchecked(self.hostname).into_owned(),
            CLASS::IN,
            ADDRESS_TTL,
            RData::A(A {
                address: self.ip.into(),
            }),
        )
        .with_cache_flush(true)
    }

    /// The service's records: the PTR from its type to its instance, the instance's SRV, of the host and port, and its
    /// TXT, of its facts, and the PTR which lists its type among the network's services.
    fn service(&self, service: &MdnsService) -> anyhow::Result<[ResourceRecord<'static>; 4]> {
        let (type_name, instance_name) = (service.type_name(), service.instance_name());
        let ptr = ResourceRecord::new(
            Name::new_unchecked(&type_name).into_owned(),
            CLASS::IN,
            SERVICE_TTL,
            RData::PTR(PTR(Name::new_unchecked(&instance_name).into_owned())),
        );
        let srv = ResourceRecord::new(
            Name::new_unchecked(&instance_name).into_owned(),
            CLASS::IN,
            ADDRESS_TTL,
            RData::SRV(SRV {
                priority: 0,
                weight: 0,
                port: service.port,
                target: Name::new_unchecked(self.hostname).into_owned(),
            }),
        )
        .with_cache_flush(true);
        let entries: Vec<String> = service
            .txt
            .snapshot()
            .0
            .into_iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect();
        let mut txt = TXT::new();
        for entry in &entries {
            txt.add_string(entry)?;
        }
        let txt = ResourceRecord::new(
            Name::new_unchecked(&instance_name).into_owned(),
            CLASS::IN,
            SERVICE_TTL,
            RData::TXT(txt.into_owned()),
        )
        .with_cache_flush(true);
        let services = ResourceRecord::new(
            Name::new_unchecked(SERVICES).into_owned(),
            CLASS::IN,
            SERVICE_TTL,
            RData::PTR(PTR(Name::new_unchecked(&type_name).into_owned())),
        );
        Ok([ptr, srv, txt, services])
    }

    /// The reply to packet's questions, None if none is for this device: its address, for its name; its service's
    /// records, for its type, its instance, or the network's services, with the rest as additional records, as DNS-SD
    /// browsers expect, so that one reply is enough.
    fn reply(&self, packet: &Packet) -> anyhow::Result<Option<Packet<'static>>> {
        let mut reply = Packet::new_reply(packet.id());
        let wants = |qtype: QTYPE, of: TYPE| qtype == QTYPE::TYPE(of) || qtype == QTYPE::ANY;
        let services = self
            .services
            .iter()
            .map(|service| Ok((service, self.service(service)?)))
            .collect::<anyhow::Result<Vec<_>>>()?;
        // The services whose records were answered, whose SRV and TXT go with them.
        let mut answered = Vec::new();
        for question in &packet.questions {
            let name = question.qname.to_string();
            if name.eq_ignore_ascii_case(self.hostname) && wants(question.qtype, TYPE::A) {
                reply.answers.push(self.address());
            }
            for (index, (service, [ptr, srv, txt, services])) in services.iter().enumerate() {
                if name.eq_ignore_ascii_case(&service.type_name()) && wants(question.qtype, TYPE::PTR) {
                    reply.answers.push(ptr.clone());
                    answered.push(index);
                } else if name.eq_ignore_ascii_case(&service.instance_name()) {
                    if wants(question.qtype, TYPE::SRV) {
                        reply.answers.push(srv.clone());
                    }
                    if wants(question.qtype, TYPE::TXT) {
                        reply.answers.push(txt.clone());
                    }
                    answered.push(index);
                } else if name.eq_ignore_ascii_case(SERVICES) && wants(question.qtype, TYPE::PTR) {
                    reply.answers.push(services.clone());
                }
            }
        }
        if reply.answers.is_empty() {
            return Ok(None);
        }
        // The rest of what a browser needs, which it would otherwise ask for, one after another.
        for index in answered {
            let [_, srv, txt, _] = &services[index].1;
            for record in [srv, txt] {
                if !reply.answers.contains(record) && !reply.additional_records.contains(record) {
                    reply.additional_records.push(record.clone());
                }
            }
        }
        let address = self.address();
        if !reply.answers.contains(&address) {
            reply.additional_records.push(address);
        }
        Ok(Some(reply))
    }

    /// Every record, announced unasked, at the start, when the service's facts change, and every ANNOUNCE_EVERY.
    fn announcement(&self) -> anyhow::Result<Packet<'static>> {
        let mut packet = Packet::new_reply(0);
        packet.answers.push(self.address());
        for service in self.services {
            packet.answers.extend(self.service(service)?);
        }
        Ok(packet)
    }
}

impl MdnsResponder {
    pub fn new(wifi_receiver: Receiver<Ipv4Addr>) -> Self {
        Self {
            wifi_receiver: Some(wifi_receiver),
        }
    }

    fn run(wifi_receiver: Receiver<Ipv4Addr>, hostname: String, services: Vec<MdnsService>) -> anyhow::Result<()> {
        let hostname_local = format!("{hostname}.local");

        info!("mDNS waiting for IP...");
        let mut ip = wifi_receiver.recv()?;
        info!("mDNS IP acquired: {}", ip);

        let socket = bind_mdns()?;
        // A timeout, so that a change to the service's facts is announced at once, rather than when a packet arrives.
        socket.set_read_timeout(Some(Duration::from_secs(1)))?;

        // IMPORTANT for ESP32/lwIP: use actual interface IP
        socket.join_multicast_v4(&"224.0.0.251".parse().unwrap(), &ip)?;
        socket.set_multicast_ttl_v4(255)?;

        let multicast: SocketAddr = "224.0.0.251:5353".parse()?;
        let send = |packet: anyhow::Result<Packet<'static>>, to: SocketAddr| match packet
            .and_then(|packet| Ok(packet.build_bytes_vec()?))
        {
            Ok(bytes) => {
                if let Err(error) = socket.send_to(&bytes, to) {
                    error!("mDNS send error: {error}");
                }
            }
            Err(error) => error!("Unable to make an mDNS packet, {error:#}"),
        };

        info!("mDNS responder started: {hostname_local} -> {ip}");
        for service in &services {
            info!("mDNS service {}", service.instance_name());
        }
        let mut buf = [0u8; 1500];
        let mut last_announce = Instant::now();
        // The TXT record's changes announced, None until the first announcement.
        let mut announced: Option<u32> = None;

        loop {
            match wifi_receiver.try_recv() {
                Ok(new_ip) if new_ip != ip => {
                    info!("mDNS IP updated: {ip} -> {new_ip}");
                    ip = new_ip;
                    announced = None;
                }
                Ok(_) | Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    error!("WiFi channel disconnected");
                }
            }
            let records = Records {
                hostname: &hostname_local,
                ip,
                services: &services,
            };

            // --- 1. Handle incoming queries ---
            match socket.recv_from(&mut buf) {
                Ok((len, from)) => {
                    if let Ok(packet) = Packet::parse(&buf[..len])
                        && !packet.questions.is_empty()
                    {
                        match records.reply(&packet) {
                            // To the asker, always, as a reply multicast on the device's network does not reach one on
                            // another, e.g. a computer on the house's main network asking a device on its IoT network,
                            // even through a router which passes mDNS between them, as seen on 23 November 2026. And
                            // to everyone too, as mDNS expects, unless it was a one off query, from another port.
                            Ok(Some(mut reply)) => {
                                if from.port() != 5353 {
                                    // A one off query's reply repeats its questions, and its records have no cache
                                    // flush bit, see RFC 6762 6.7.
                                    reply.questions = packet.questions.iter().map(|q| q.clone().into_owned()).collect();
                                    for record in reply.answers.iter_mut().chain(reply.additional_records.iter_mut()) {
                                        record.cache_flush = false;
                                    }
                                    send(Ok(reply), from);
                                } else {
                                    send(Ok(reply.clone()), from);
                                    send(Ok(reply), multicast);
                                }
                            }
                            Ok(None) => {}
                            Err(error) => error!("Unable to answer an mDNS query, {error:#}"),
                        }
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(error) => {
                    error!("mDNS recv error: {error}");
                }
            }

            // --- 2. Announcements: at the start, when the service's facts change, and now and then, for caches ---
            let changes: u32 = services.iter().map(|service| service.txt.snapshot().1).sum();
            if announced != Some(changes) || last_announce.elapsed() > ANNOUNCE_EVERY {
                send(records.announcement(), multicast);
                info!("mDNS announced {hostname_local}");
                announced = Some(changes);
                last_announce = Instant::now();
            }
        }
    }

    /// Starts answering for hostname, `<hostname>.local`, and advertising services, on a thread of its own, once the
    /// Wi-Fi has an address.
    pub fn start(&mut self, hostname: String, services: Vec<MdnsService>) -> Result<()> {
        let Some(wifi_receiver) = self.wifi_receiver.take() else {
            anyhow::bail!("Receiver is None");
        };
        // Not the default stack, 16 KB on some boards, as it only needs room for a packet, and the RAM is needed for
        // HTTPS connections.
        thread::Builder::new()
            .stack_size(crate::thread_stack(MDNS_STACK))
            .spawn(move || {
                if let Err(error) = Self::run(wifi_receiver, hostname, services) {
                    error!("mDNS error: {}", error);
                }
            })?;
        Ok(())
    }
}

/// The socket mDNS is answered on, port 5353, which on a device is the responder's alone.
#[cfg(target_os = "espidf")]
fn bind_mdns() -> anyhow::Result<UdpSocket> {
    Ok(UdpSocket::bind("0.0.0.0:5353")?)
}

/// The socket mDNS is answered on, port 5353, which on a computer, e.g. in the board simulator, the computer's own
/// responder has, so shared with it, as every responder on a computer does.
#[cfg(not(target_os = "espidf"))]
fn bind_mdns() -> anyhow::Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;
    socket.bind(&std::net::SocketAddr::from(([0, 0, 0, 0], 5353)).into())?;
    Ok(socket.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use simple_dns::{QCLASS, Question};

    fn service() -> MdnsService {
        let txt = MdnsTxt::default();
        txt.set("id", "44bd8d791738");
        txt.set("board", "wave-esp32s3-lcd-147");
        MdnsService {
            service_type: SERVICE_TYPE.to_string(),
            instance: "44bd8d791738".to_string(),
            port: 80,
            txt,
        }
    }

    fn query(name: &str, qtype: TYPE) -> Packet<'static> {
        let mut packet = Packet::new_query(7);
        packet.questions.push(
            Question::new(
                Name::new_unchecked(name).into_owned(),
                QTYPE::TYPE(qtype),
                QCLASS::CLASS(CLASS::IN),
                false,
            )
            .into_owned(),
        );
        packet
    }

    #[test]
    fn a_browser_asking_for_the_type_gets_the_instance_with_its_port_facts_and_address() {
        let service = service();
        let records = Records {
            hostname: "wave147s3.local",
            ip: Ipv4Addr::new(192, 168, 4, 84),
            services: std::slice::from_ref(&service),
        };
        let reply = records
            .reply(&query("_bruce-device._tcp.local", TYPE::PTR))
            .unwrap()
            .unwrap();
        assert!(
            matches!(&reply.answers[0].rdata, RData::PTR(PTR(name)) if name.to_string() == "44bd8d791738._bruce-device._tcp.local")
        );
        let srv = reply.additional_records.iter().find_map(|record| match &record.rdata {
            RData::SRV(srv) => Some(srv.clone()),
            _ => None,
        });
        assert_eq!(
            srv.map(|srv| (srv.port, srv.target.to_string())),
            Some((80, "wave147s3.local".to_string()))
        );
        let txt = reply.additional_records.iter().find_map(|record| match &record.rdata {
            RData::TXT(txt) => Some(txt.attributes()),
            _ => None,
        });
        let txt = txt.unwrap();
        assert_eq!(txt.get("board"), Some(&Some("wave-esp32s3-lcd-147".to_string())));
        assert!(
            reply
                .additional_records
                .iter()
                .any(|record| matches!(record.rdata, RData::A(_)))
        );
        // It parses as a packet again, as a browser reads it.
        Packet::parse(&reply.build_bytes_vec().unwrap()).unwrap();
    }

    #[test]
    fn a_device_is_found_under_each_of_its_types_with_the_same_facts() {
        let bruce = service();
        let http = MdnsService {
            service_type: HTTP_SERVICE_TYPE.to_string(),
            ..bruce.clone()
        };
        let services = [bruce, http];
        let records = Records {
            hostname: "wave147s3.local",
            ip: Ipv4Addr::new(192, 168, 4, 84),
            services: &services,
        };
        for type_name in ["_bruce-device._tcp.local", "_http._tcp.local"] {
            let reply = records.reply(&query(type_name, TYPE::PTR)).unwrap().unwrap();
            assert_eq!(reply.answers.len(), 1, "{type_name}");
            let txt = reply
                .additional_records
                .iter()
                .filter(|record| matches!(record.rdata, RData::TXT(_)))
                .count();
            assert_eq!(txt, 1, "only its own instance's facts, for {type_name}");
        }
    }

    #[test]
    fn a_change_to_the_facts_is_counted_once_and_a_query_for_something_else_is_not_answered() {
        let service = service();
        let before = service.txt.snapshot().1;
        service.txt.set("adopted", "1");
        service.txt.set("adopted", "1");
        assert_eq!(service.txt.snapshot().1, before + 1);
        let records = Records {
            hostname: "wave147s3.local",
            ip: Ipv4Addr::new(192, 168, 4, 84),
            services: std::slice::from_ref(&service),
        };
        assert!(
            records
                .reply(&query("_googlecast._tcp.local", TYPE::PTR))
                .unwrap()
                .is_none()
        );
        let reply = records
            .reply(&query("44bd8d791738._bruce-device._tcp.local", TYPE::TXT))
            .unwrap()
            .unwrap();
        assert!(matches!(reply.answers[0].rdata, RData::TXT(_)));
    }
}
