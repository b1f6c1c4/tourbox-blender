#![feature(generic_arg_infer)]

use std::{error::Error, fmt::Display, env};
use std::net::{IpAddr, SocketAddr, UdpSocket};

use bluer::{
    gatt::remote::{Characteristic, Descriptor, Service},
    Adapter, AdapterEvent, Address, Device, Uuid,
};
use clap::{arg, command, value_parser};
use futures::stream::StreamExt;
use futures::{future::Shared, Future, FutureExt};
use input::TourboxInput;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::signal::unix::{signal, SignalKind};

type TBResult<T> = Result<T, Box<dyn Error>>;

const UUID_TOURBOX_SERVICE: Uuid = Uuid::from_u128(0xfff000001000800000805f9b34fb);
const UUID_CHAR0011: Uuid = Uuid::from_u128(0xfff300001000800000805f9b34fb);
const UUID_CHAR0011_DESC0013: Uuid = Uuid::from_u128(0x290200001000800000805f9b34fb);
const UUID_CHAR000F: Uuid = Uuid::from_u128(0xfff200001000800000805f9b34fb);
const UUID_CHAR000C: Uuid = Uuid::from_u128(0xfff100001000800000805f9b34fb);
const UUID_CHAR000C_DESC000E: Uuid = Uuid::from_u128(0x290200001000800000805f9b34fb);

mod input;

#[derive(Debug)]
struct ShutdownError;
impl Display for ShutdownError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Interupted by a shutdown signal")
    }
}
impl Error for ShutdownError {}
impl ShutdownError {
    fn new<T>() -> TBResult<T> {
        Err(Box::new(ShutdownError))
    }
}

pub struct Tourbox<F>
where
    F: Future<Output = ()>,
{
    pub shutdown: Shared<F>,
    pub device: Device,
    pub service: Service,
    pub char0011: Characteristic,
    pub char0011_desc0013: Descriptor,
    pub char000f: Characteristic,
    pub char000c: Characteristic,
    pub char000c_desc000e: Descriptor,
}

impl<F: Future<Output = ()>> Tourbox<F> {
    pub async fn start_server(
        addr: Address,
        adapter: Adapter,
        shutdown: F,
    ) -> TBResult<Tourbox<F>> {
        let device = adapter.device(addr).unwrap();
        device.connect().await.unwrap();
        let shutdown = shutdown.shared();
        let construct = async {
            let shutdown = shutdown.clone();
            let service = find_service(&device, UUID_TOURBOX_SERVICE).await;
            let char0011 = find_characteristic(&service, UUID_CHAR0011).await;
            let char0011_desc0013 = find_descriptor(&char0011, UUID_CHAR0011_DESC0013).await;
            let char000f = find_characteristic(&service, UUID_CHAR000F).await;
            let char000c = find_characteristic(&service, UUID_CHAR000C).await;
            let char000c_desc000e = find_descriptor(&char000c, UUID_CHAR000C_DESC000E).await;

            return Tourbox {
                shutdown,
                device,
                service,
                char0011,
                char0011_desc0013,
                char000f,
                char000c,
                char000c_desc000e,
            };
        };
        tokio::select! {
            tb = construct => Ok(tb),
            _ = shutdown.clone() => ShutdownError::new(),
        }
    }
    pub async fn initial_protocol(&mut self) -> TBResult<()> {
        let mut writer = self.char000f.write_io().await?;
        let line_1: [u8; _] = [0x55, 0x00, 0x07, 0x88, 0x94, 0x00, 0x1a, 0xfe];
        let line_2: [u8; _] = [
            0xb5, 0x00, 0x5d, 0x04, 0x08, 0x05, 0x08, 0x06, 0x08, 0x07, 0x08, 0x08, 0x08, 0x09,
            0x08, 0x0b, 0x08, 0x0c, 0x08, 0x0d,
        ];
        let line_3: [u8; _] = [
            0x08, 0x0e, 0x08, 0x0f, 0x08, 0x26, 0x08, 0x27, 0x08, 0x28, 0x08, 0x29, 0x08, 0x3b,
            0x08, 0x3c, 0x08, 0x3d, 0x08, 0x3e,
        ];
        let line_4: [u8; _] = [
            0x08, 0x3f, 0x08, 0x40, 0x08, 0x41, 0x08, 0x42, 0x08, 0x43, 0x08, 0x44, 0x08, 0x45,
            0x08, 0x46, 0x08, 0x47, 0x08, 0x48,
        ];
        let line_5: [u8; _] = [
            0x08, 0x49, 0x08, 0x4a, 0x08, 0x4b, 0x08, 0x4c, 0x08, 0x4d, 0x08, 0x4e, 0x08, 0x4f,
            0x08, 0x50, 0x08, 0x51, 0x08, 0x52,
        ];
        let line_6: [u8; _] = [
            0x08, 0x53, 0x08, 0x54, 0x08, 0xa8, 0x08, 0xa9, 0x08, 0xaa, 0x08, 0xab, 0x08, 0xfe,
        ];
        let writing = async {
            writer.write_all(&line_1).await?;
            writer.write_all(&line_2).await?;
            writer.write_all(&line_3).await?;
            writer.write_all(&line_4).await?;
            writer.write_all(&line_5).await?;
            writer.write_all(&line_6).await?;
            Ok(()) as TBResult<_>
        };
        tokio::select! {
            _ = writing => Ok(()),
            _ = self.shutdown.clone() => ShutdownError::new(),
        }
    }
    pub async fn notifications<E>(&mut self, func: E) -> TBResult<()>
        where E: Fn(TourboxInput) -> () {
        let mut buffer = [0u8; 4096];
        eprintln!("Listening for events...");
        loop {
            // Notifier buffers data, rebuild it each iteration
            // Putting this inside the loop seems to greatly reduce the number of wheel events
            // There's something not working with how CharacteristicReader buffers things for async reads
            let mut notifier = self.char000c.notify_io().await?;
            // Note that this will only flush the fd, not the library's async buffer
            let amount = if let Ok(data) = notifier.try_recv() {
                eprintln!("Discarded {} bytes (from fd)", data.len());
                continue;
            } else {
                tokio::select! {
                    amount = notifier.read(&mut buffer) => {amount}
                    _ = self.shutdown.clone() => {
                        return Ok(());
                    }
                }?
            };
            let event = if amount == 1 {
                TourboxInput::from_u8(buffer[0])
            } else if amount == 2 {
                TourboxInput::from_u16(u16::from_be_bytes([buffer[0], buffer[1]]))
            } else {
                eprintln!("Discarded {} bytes (read too much)", amount);
                continue;
            };
            func(event);
        }
    }
}

async fn find_service(device: &Device, uuid: Uuid) -> Service {
    for service in device.services().await.unwrap() {
        if service.uuid().await.unwrap() == uuid {
            return service;
        }
    }
    panic!("Service not found! {}", uuid);
}

async fn find_characteristic(service: &Service, uuid: Uuid) -> Characteristic {
    for characteristic in service.characteristics().await.unwrap() {
        if characteristic.uuid().await.unwrap() == uuid {
            return characteristic;
        }
    }
    panic!("Characteristic not found! {}", uuid);
}

async fn find_descriptor(characteristic: &Characteristic, uuid: Uuid) -> Descriptor {
    for descriptor in characteristic.descriptors().await.unwrap() {
        if descriptor.uuid().await.unwrap() == uuid {
            return descriptor;
        }
    }
    panic!("Descriptor not found! {}", uuid);
}

#[tokio::main]
async fn main() -> TBResult<()> {
    let matches = command!()
        .version("0.1.0")
        .author("Author Name <email@example.com>")
        .about("Stream data from TourBox Elite to UDP.")
        .arg(
            arg!(-u <IP> "IP address to which I'll send UDP packages")
                .value_parser(value_parser!(IpAddr))
                .default_value("127.0.0.1"),
        )
        .arg(
            arg!(-p <PORT> "Port to which I'll send UDP packages")
                .value_parser(value_parser!(u16))
                .default_value("21404"),
        )
        .arg(
            arg!(<ADDR> "Sets the Bluetooth MAC address of the device")
                .required(true)
                .index(1)
                .value_parser(value_parser!(bluer::Address)),
        )
        .get_matches();

    let socket_addr = SocketAddr::from((
            *matches.get_one::<IpAddr>("IP").unwrap(),
            *matches.get_one::<u16>("PORT").unwrap(),
    ));
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.connect(socket_addr)?;
    let emitter = |e: TourboxInput| {
        let str = e.to_string();
        socket.send(str.as_bytes()).unwrap_or(0);
    };

    let device_addr = *matches.get_one::<bluer::Address>("ADDR").expect("");

    let stop = async {
        signal(SignalKind::interrupt()).unwrap().recv().await;
    }
    .shared();

    let startup = async {
        let session = bluer::Session::new().await?;
        let adapter = session.default_adapter().await?;
        adapter.set_powered(true).await?;
        let mut devices = adapter.discover_devices().await?;
        eprintln!("Scanning...");
        while let Some(device) = devices.next().await {
            if let AdapterEvent::DeviceAdded(addr) = device {
                if addr == device_addr {
                    return Ok(adapter);
                }
            }
        }
        TBResult::<_>::Err(Box::new(ShutdownError))
    };
    let adapter = tokio::select! {
        _ = stop.clone() => {
            eprintln!("Exited before startup?");
            return ShutdownError::new();
        },
        result = startup => result?,
    };
    let mut tb = Tourbox::start_server(device_addr, adapter, stop).await?;
    eprintln!("Device connected! :)");
    tb.initial_protocol().await?;
    tb.notifications(emitter).await?;

    eprintln!("Exited cleanly");
    Ok(())
}
