#![allow(dead_code)]

use std::fmt::{Debug, Formatter};
use std::io::Write;
use std::time::Duration;

use anyhow::{anyhow, bail, Context};
use rusb::{Device, DeviceDescriptor, UsbContext};

use crate::probe::ti_icdi::hla_interface::{HlaInterface, ICDI_MAX_PACKET_SIZE};
use crate::probe::ti_icdi::receive_buffer::ReceiveBuffer;
use crate::{
    DebugProbeError, DebugProbeInfo, DebugProbeSelector, DebugProbeType, ProbeCreationError,
};

const ICDI_VID: u16 = 0x1cbe;
const ICDI_PID: u16 = 0x00fd;

const INTERFACE_NR: u8 = 0x02;

pub(super) const ICDI_READ_ENDPOINT: u8 = 0x83;
pub(super) const ICDI_WRITE_ENDPOINT: u8 = 0x02;

pub(super) const TIMEOUT: Duration = Duration::from_secs(1);

pub fn list_icdi_devices() -> Vec<DebugProbeInfo> {
    rusb::Context::new()
        .and_then(|ctx| ctx.devices())
        .map(|devices| {
            devices
                .iter()
                .filter(is_icdi_device)
                .filter_map(|device| {
                    let descr = device.device_descriptor().ok()?;
                    let serial = read_serial_number(&device, &descr);
                    Some(DebugProbeInfo::new(
                        format!("TI-ICDI {}", "<TODO>"),
                        descr.vendor_id(),
                        descr.product_id(),
                        serial,
                        DebugProbeType::Icdi,
                    ))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|_| Vec::new())
}

fn is_icdi_device<U: UsbContext>(device: &Device<U>) -> bool {
    device.device_descriptor().map_or(false, |descr| {
        descr.vendor_id() == ICDI_VID && descr.product_id() == ICDI_PID
    })
}

fn read_serial_number<U: UsbContext>(
    device: &Device<U>,
    descriptor: &DeviceDescriptor,
) -> Option<String> {
    device
        .open()
        .ok()?
        .read_string_descriptor_ascii(descriptor.serial_number_string_index()?)
        .ok()
}

pub struct IcdiUsbInterface {
    device: rusb::DeviceHandle<rusb::Context>,
}

impl Debug for IcdiUsbInterface {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "IcdiUsbInterface: <..>")
    }
}

impl IcdiUsbInterface {
    pub fn new_from_selector(
        selector: impl Into<DebugProbeSelector>,
    ) -> Result<Self, ProbeCreationError> {
        let selector = selector.into();
        let ctx = rusb::Context::new()?;
        let device = ctx
            .devices()?
            .iter()
            .filter(is_icdi_device)
            .find_map(|device| {
                let descr = device.device_descriptor().ok()?;
                if selector.vendor_id != descr.vendor_id()
                    || selector.product_id != descr.product_id()
                {
                    return None;
                }
                if selector.serial_number.is_none()
                    || selector.serial_number == read_serial_number(&device, &descr)
                {
                    Some(device)
                } else {
                    None
                }
            })
            .map_or(Err(ProbeCreationError::NotFound), Ok)?;

        let mut handle = device.open()?;
        handle.claim_interface(INTERFACE_NR)?;

        let mut interface = Self { device: handle };

        interface
            .init()
            .map_err(|_| ProbeCreationError::Other("Init failed"))?;

        Ok(interface)
    }

    fn init(&mut self) -> Result<(), DebugProbeError> {
        self.send_cmd(b"!")?.check_cmd_result()
    }

    fn send_remote_cmd(&mut self, data: &[u8]) -> Result<ReceiveBuffer, DebugProbeError> {
        self.custom_command(data)
    }
}

pub(super) fn write_hex(mut w: impl Write, data: &[u8]) {
    for byte in data {
        write!(w, "{:02x}", byte).expect("Hexify write failed");
    }
}

pub(super) fn new_send_buffer(capacity: usize) -> Vec<u8> {
    let mut b = Vec::with_capacity(capacity + 4);
    b.push(b'$');
    b
}

impl HlaInterface for IcdiUsbInterface {
    fn reset(&mut self) -> Result<(), DebugProbeError> {
        todo!()
    }

    fn read_mem_int(
        &mut self,
        addr: u32,
        len: u32,
        buf: &mut Vec<u8>,
    ) -> Result<(), DebugProbeError> {
        let mut buf = new_send_buffer(20);
        write!(&mut buf, "x%{:08x},{:08x}", addr, len).unwrap();
        let response = self.send_cmd(&buf)?;
        response.check_cmd_result()?;
        buf.reserve(response.len() - 8);
        let mut escaped = false;
        for &byte in &response[5..response.len() - 3] {
            if escaped {
                buf.push(byte ^ 0x20);
                escaped = false;
            } else if byte == b'}' {
                escaped = true;
            } else {
                buf.push(byte)
            }
        }
        Ok(())
    }

    fn write_mem_int(&mut self, addr: u32, data: &[u8]) -> Result<(), DebugProbeError> {
        let mut buf = new_send_buffer(11 + data.len());
        write!(&mut buf, "X[{:08x},{:08x}:", addr, data.len()).unwrap();
        for &byte in data {
            match byte {
                b'$' | b'#' | b'}' | b'*' => {
                    buf.push(b'}');
                    buf.push(byte ^ 0x20);
                }
                _ => buf.push(byte),
            }
        }
        self.send_packet(buf)?.check_cmd_result()
    }

    fn send_packet(&mut self, mut data: Vec<u8>) -> Result<ReceiveBuffer, DebugProbeError> {
        assert_eq!(data[0], b'$');
        let checksum = data
            .iter()
            .skip(1)
            .fold(0u8, |acc, &byte| acc.wrapping_add(byte));
        write!(&mut data, "#{:02x}", checksum).expect("ICDI buffer write failed.");
        for _retries in 0..3 {
            let sent = self
                .device
                .write_bulk(ICDI_WRITE_ENDPOINT, &data, TIMEOUT)
                .context("ICDI USB write failed.")?;
            if sent != data.len() {
                return Err(anyhow!("ICDI buffer wasn't sent completely.").into());
            }

            let buf = ReceiveBuffer::from_bulk_receive(&mut self.device, TIMEOUT)?;
            if buf.len() < 1 {
                return Err(anyhow!("ICDI zero length response").into());
            }
            match buf[0] {
                b'-' => {
                    log::trace!("Resending packet");
                    continue;
                }
                b'+' => return Ok(buf), // FIXME: openocd does extra reads
                _ => {
                    log::trace!("Unexpected response from ICDI {:?}", buf)
                }
            }
        }
        Err(anyhow!("Too many retires").into())
    }
}
