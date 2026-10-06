//! Network bring-up and discovery. A controller joins its saved network, or
//! hosts its own open access point when it has none or cannot join it. Either
//! way it advertises `_donder._tcp` over mDNS so the editor can find it.
use core::cell::RefCell;
use core::fmt::Write as _;
use core::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4};

use donder_device_storage::device_config::{DeviceConfig, MAX_NAME_BYTES, Network};
use edge_mdns::domain::base::Ttl;
use edge_mdns::host::{Host, Service, ServiceAnswers};
use edge_mdns::{HostAnswer, HostAnswers, HostAnswersMdnsHandler, MdnsError};
use edge_nal::{UdpBind, UdpSplit};
use embassy_net::{Ipv4Cidr, Stack, StaticConfigV4};
use embassy_sync::blocking_mutex::{
    Mutex,
    raw::{CriticalSectionRawMutex, NoopRawMutex},
};
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Timer, with_timeout};
use esp_radio::wifi::{
    self, AuthenticationMethodConfig, Config, Interface, WifiController, ap::AccessPointConfig,
    sta::StationConfig,
};
use static_cell::StaticCell;

pub const ACCESS_POINT_ADDRESS: Ipv4Addr = Ipv4Addr::new(192, 168, 4, 1);
const STATION_JOIN_TIMEOUT: Duration = Duration::from_secs(20);

pub type Name = heapless::String<MAX_NAME_BYTES>;

/// The controller's mutable identity, shared by HTTP, clock and mDNS tasks.
/// Flash holds the saved copy; this is updated only after a save succeeds.
pub struct Identity {
    pub name: Name,
    pub token: Option<[u8; 32]>,
}

pub static IDENTITY: Mutex<CriticalSectionRawMutex, RefCell<Identity>> =
    Mutex::new(RefCell::new(Identity {
        name: heapless::String::new(),
        token: None,
    }));
/// Re-announce mDNS records after the name or claim changes.
pub static ANNOUNCE: Signal<CriticalSectionRawMutex, ()> = Signal::new();
/// Restart after a network change has been acknowledged to the editor.
pub static RESTART: Signal<CriticalSectionRawMutex, ()> = Signal::new();

pub fn token() -> Option<[u8; 32]> {
    IDENTITY.lock(|identity| identity.borrow().token)
}

/// Stable controller identity: the factory MAC address in lowercase hex.
pub fn device_id() -> heapless::String<12> {
    let mut id = heapless::String::new();
    for byte in esp_hal::efuse::base_mac_address().as_bytes() {
        write!(id, "{byte:02x}").unwrap();
    }
    id
}

/// The access point keeps this name through renames, so a computer that
/// joined it once rejoins it after a restart.
fn access_point_ssid() -> heapless::String<11> {
    let id = device_id();
    let mut ssid = heapless::String::new();
    write!(ssid, "Donder-{}", &id[8..]).unwrap();
    ssid
}

pub fn default_config() -> DeviceConfig {
    DeviceConfig {
        name: access_point_ssid().as_str().into(),
        network: None,
        token: None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Station,
    AccessPoint,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Station => "station",
            Self::AccessPoint => "accessPoint",
        }
    }
}

/// Join the saved network, falling back to the controller's own access point.
pub async fn start(
    device: esp_hal::peripherals::WIFI<'static>,
    config: &DeviceConfig,
) -> (WifiController<'static>, Interface, Mode) {
    let access_point = Config::AccessPoint(
        AccessPointConfig::default()
            .with_ssid(access_point_ssid().as_str().try_into().unwrap())
            .with_authentication(AuthenticationMethodConfig::Open),
    );
    let initial = config
        .network
        .as_ref()
        .map_or_else(|| access_point.clone(), station);
    // Four static receive buffers (the default is ten, about 1.6 KiB each)
    // leave that heap to shows; control traffic is light.
    let mut controller = WifiController::new(
        device,
        wifi::ControllerConfig::default()
            .with_static_rx_buf_num(4)
            .with_initial_config(initial),
    )
    .unwrap();
    controller
        .set_power_saving(wifi::PowerSaveMode::None)
        .unwrap();
    if config.network.is_some() {
        match with_timeout(STATION_JOIN_TIMEOUT, join(&mut controller)).await {
            Ok(()) => return (controller, Interface::station(), Mode::Station),
            Err(_) => {
                println!("WIFI JOIN FAILED; starting access point");
                controller.set_config(&access_point).unwrap();
            }
        }
    }
    (controller, Interface::access_point(), Mode::AccessPoint)
}

fn station(network: &Network) -> Config {
    Config::Station(
        StationConfig::default()
            .with_ssid(network.ssid.as_str().try_into().unwrap())
            .with_authentication(AuthenticationMethodConfig::Wpa2Personal(
                network.password.as_str().try_into().unwrap(),
            )),
    )
}

async fn join(controller: &mut WifiController<'static>) {
    while controller.connect_async().await.is_err() {
        Timer::after_secs(1).await;
    }
}

pub fn stack_config(mode: Mode) -> embassy_net::Config {
    match mode {
        Mode::Station => embassy_net::Config::dhcpv4(Default::default()),
        Mode::AccessPoint => embassy_net::Config::ipv4_static(StaticConfigV4 {
            address: Ipv4Cidr::new(ACCESS_POINT_ADDRESS, 24),
            gateway: None,
            dns_servers: Default::default(),
        }),
    }
}

#[embassy_executor::task]
pub async fn reconnect(mut controller: WifiController<'static>, mode: Mode) {
    if mode == Mode::AccessPoint {
        // The access point runs without supervision; keep the controller alive.
        core::future::pending::<()>().await;
    }
    loop {
        if controller.is_connected() {
            let _ = controller.wait_for_disconnect_async().await;
            println!("WIFI DISCONNECTED");
        }
        match controller.connect_async().await {
            Ok(_) => println!("WIFI CONNECTED"),
            Err(_) => {
                println!("WIFI RETRY");
                Timer::after_secs(5).await;
            }
        }
    }
}

#[embassy_executor::task]
pub async fn restart_after_change() {
    RESTART.wait().await;
    // Let the HTTP response leave before the radio goes down.
    Timer::after_millis(500).await;
    esp_hal::system::software_reset();
}

// Network buffers live in the ESP32's 8 KiB RTC fast RAM. Only core 0 can
// address it, and only core 0 runs the network, which leaves main DRAM to the
// core 0 stack and the show heap.
pub type UdpPool = edge_nal_embassy::UdpBuffers<2, 768, 768, 2>;
type MdnsBuffer = edge_mdns::buf::VecBufAccess<NoopRawMutex, 768>;
#[esp_hal::ram(unstable(rtc_fast))]
static UDP_POOL: StaticCell<UdpPool> = StaticCell::new();
#[esp_hal::ram(unstable(rtc_fast))]
static MDNS_BUFFERS: StaticCell<(MdnsBuffer, MdnsBuffer)> = StaticCell::new();
#[esp_hal::ram(unstable(rtc_fast))]
static DHCP_BUFFER: StaticCell<[u8; 768]> = StaticCell::new();

pub fn udp_pool() -> &'static UdpPool {
    UDP_POOL.init(UdpPool::new())
}

/// Leases addresses on the access point. No gateway is offered, so an editor
/// on another network (ethernet) keeps its internet route.
#[embassy_executor::task]
pub async fn dhcp_server(stack: Stack<'static>, pool: &'static UdpPool) -> ! {
    use edge_dhcp::io::{DEFAULT_SERVER_PORT, server};
    use edge_dhcp::server::{Server, ServerOptions};
    let udp = edge_nal_embassy::Udp::new(stack, pool);
    let mut socket = udp
        .bind(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::UNSPECIFIED,
            DEFAULT_SERVER_PORT,
        )))
        .await
        .unwrap();
    let buffer = DHCP_BUFFER.init([0; 768]);
    let mut leases = Server::<_, 4>::new_with_et(ACCESS_POINT_ADDRESS);
    let options = ServerOptions::new(ACCESS_POINT_ADDRESS, None);
    loop {
        if let Err(error) = server::run(&mut leases, &options, &mut socket, buffer).await {
            println!("DHCP server failed: {:?}", error);
            Timer::after_secs(1).await;
        }
    }
}

struct Answers {
    stack: Stack<'static>,
    hostname: &'static str,
    id: &'static str,
    mode: Mode,
}

impl HostAnswers for Answers {
    fn visit<F, E>(&self, f: F) -> Result<(), E>
    where
        F: FnMut(HostAnswer) -> Result<(), E>,
        E: From<MdnsError>,
    {
        let Some(config) = self.stack.config_v4() else {
            return Ok(());
        };
        let (name, claimed) = IDENTITY.lock(|identity| {
            let identity = identity.borrow();
            (identity.name.clone(), identity.token.is_some())
        });
        let mut format = heapless::String::<10>::new();
        write!(format, "{}", donder_runtime::FORMAT_VERSION).unwrap();
        let host = Host {
            hostname: self.hostname,
            ipv4: config.address.address(),
            ipv6: Ipv6Addr::UNSPECIFIED,
            ttl: Ttl::from_secs(60),
        };
        let txt = [
            ("id", self.id),
            ("name", name.as_str()),
            ("claimed", if claimed { "1" } else { "0" }),
            ("format", format.as_str()),
            ("network", self.mode.label()),
        ];
        let service = Service {
            name: self.hostname,
            priority: 0,
            weight: 0,
            service: "_donder",
            protocol: "_tcp",
            port: crate::HTTP_PORT,
            service_subtypes: &[],
            txt_kvs: &txt,
        };
        ServiceAnswers::new(&host, &service).visit(f)
    }
}

#[embassy_executor::task]
pub async fn mdns_responder(
    stack: Stack<'static>,
    pool: &'static UdpPool,
    id: &'static str,
    hostname: &'static str,
    mode: Mode,
) -> ! {
    use edge_mdns::io::{IPV4_DEFAULT_SOCKET, Mdns, bind};
    stack.wait_config_up().await;
    let udp = edge_nal_embassy::Udp::new(stack, pool);
    let mut socket = bind(&udp, IPV4_DEFAULT_SOCKET, Some(Ipv4Addr::UNSPECIFIED), None)
        .await
        .unwrap();
    let (receive, send) = socket.split();
    let (receive_buffer, send_buffer) = MDNS_BUFFERS.init((MdnsBuffer::new(), MdnsBuffer::new()));
    let mdns = Mdns::<_, _, _, _, _, CriticalSectionRawMutex>::new(
        Some(Ipv4Addr::UNSPECIFIED),
        None,
        receive,
        send,
        &*receive_buffer,
        &*send_buffer,
        esp_hal::rng::Rng::new(),
        &ANNOUNCE,
    );
    let answers = Answers {
        stack,
        hostname,
        id,
        mode,
    };
    loop {
        if let Err(error) = mdns.run(HostAnswersMdnsHandler::new(&answers)).await {
            println!("mDNS responder failed: {:?}", error);
            Timer::after_secs(1).await;
        }
    }
}
