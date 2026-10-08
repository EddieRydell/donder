//! Finds Donder controllers that advertise `_donder._tcp` over mDNS, on the
//! controller's own access point or any shared network.
use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use donder_model::DonderDeviceId;
use mdns_sd::{ResolvedService, ServiceDaemon, ServiceEvent};

use crate::desktop_state::lock_unpoisoned;
use donder_sequence_api::DonderDeviceNetwork;

const SERVICE: &str = "_donder._tcp.local.";

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Advertised {
    pub name: String,
    pub address: SocketAddr,
    pub claimed: bool,
    pub format: u32,
    pub network: DonderDeviceNetwork,
}

pub(crate) type Advertisements = BTreeMap<DonderDeviceId, Advertised>;

pub(crate) struct DeviceDiscovery {
    devices: Arc<Mutex<Advertisements>>,
    daemon: Result<ServiceDaemon, String>,
}

impl DeviceDiscovery {
    /// `on_change` runs on the discovery thread after the device set changes.
    pub(crate) fn new(on_change: impl Fn() + Send + 'static) -> Self {
        let devices = Arc::new(Mutex::new(Advertisements::new()));
        let started = ServiceDaemon::new().and_then(|daemon| {
            let events = daemon.browse(SERVICE)?;
            Ok((daemon, events))
        });
        let daemon = match started {
            Ok((daemon, events)) => {
                let shared = devices.clone();
                std::thread::spawn(move || {
                    let mut instances = HashMap::<String, DonderDeviceId>::new();
                    while let Ok(event) = events.recv() {
                        let changed = match event {
                            ServiceEvent::ServiceResolved(service) => {
                                parse(&service).is_some_and(|(id, advertised)| {
                                    instances.insert(service.get_fullname().into(), id.clone());
                                    lock_unpoisoned(&shared).insert(id, advertised.clone())
                                        != Some(advertised)
                                })
                            }
                            ServiceEvent::ServiceRemoved(_, fullname) => instances
                                .remove(&fullname)
                                .is_some_and(|id| lock_unpoisoned(&shared).remove(&id).is_some()),
                            _ => false,
                        };
                        if changed {
                            on_change();
                        }
                    }
                });
                Ok(daemon)
            }
            Err(error) => Err(format!("Controller discovery is unavailable: {error}")),
        };
        Self { devices, daemon }
    }

    pub(crate) fn devices(&self) -> Advertisements {
        lock_unpoisoned(&self.devices).clone()
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.daemon.as_ref().err().map(String::as_str)
    }
}

impl Drop for DeviceDiscovery {
    fn drop(&mut self) {
        if let Ok(daemon) = &self.daemon {
            let _ = daemon.shutdown();
        }
    }
}

fn parse(service: &ResolvedService) -> Option<(DonderDeviceId, Advertised)> {
    let id = DonderDeviceId::parse(service.get_property_val_str("id")?)?;
    let ip = service.get_addresses_v4().into_iter().min()?;
    Some((
        id,
        Advertised {
            name: service.get_property_val_str("name")?.to_string(),
            address: SocketAddr::new(IpAddr::V4(ip), service.get_port()),
            claimed: match service.get_property_val_str("claimed")? {
                "1" => true,
                "0" => false,
                _ => return None,
            },
            format: service.get_property_val_str("format")?.parse().ok()?,
            network: match service.get_property_val_str("network")? {
                "accessPoint" => DonderDeviceNetwork::AccessPoint,
                "station" => DonderDeviceNetwork::Station,
                _ => return None,
            },
        },
    ))
}
