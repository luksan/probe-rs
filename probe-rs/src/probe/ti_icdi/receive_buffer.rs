use anyhow::{anyhow, bail, Context, Result};
use rusb::{DeviceHandle, UsbContext};
use std::fmt::Debug;
use std::ops::Deref;
use std::time::Duration;

use crate::probe::ti_icdi::usb_interface::ICDI_READ_ENDPOINT;
use crate::DebugProbeError;

#[derive(Clone)]
pub struct ReceiveBuffer {
    data: Box<[u8]>,
    len: usize,
    decoded: bool,
}

impl ReceiveBuffer {
    fn new() -> Self {
        Self {
            data: vec![0u8; 2048].into_boxed_slice(),
            len: 0,
            decoded: false,
        }
    }

    pub fn from_bulk_receive<C: UsbContext>(
        device: &mut DeviceHandle<C>,
        timeout: Duration,
    ) -> Result<Self> {
        let mut buf = Self::new();
        let mut len = 0;
        while len < 3 || buf.data[len - 3] != b'#' {
            let slice = &mut buf.data[len..];
            if slice.is_empty() {
                bail!("Buffer couldn't hold the full response.")
            }
            len += device
                .read_bulk(ICDI_READ_ENDPOINT, slice, timeout)
                .context("Error receiving data")?;
        }
        buf.len = len;
        Ok(buf)
    }

    pub fn get_payload(&self) -> Result<&[u8], DebugProbeError> {
        let start = self.iter().position(|&c| c == b'$');
        let end = self.iter().rposition(|&c| c == b'#');
        if let (Some(start), Some(end)) = (start, end) {
            Ok(&self[start + 1..end])
        } else {
            Err(anyhow!("Malformed ICDI response").into())
        }
    }

    pub fn check_cmd_result(&self) -> Result<(), DebugProbeError> {
        let payload = self.get_payload()?;
        if payload.is_empty() {
            return Err(anyhow!("Empty response payload").into());
        }
        if payload.starts_with(b"OK") {
            Ok(())
        } else {
            if payload[0] == b'E' {
                let err = std::str::from_utf8(&payload[1..3])
                    .context("Err HEX not UTF-8")
                    .map(|s| {
                        u8::from_str_radix(s, 16).with_context(|| {
                            format!("Error code decode error, {:?}", &payload[1..3])
                        })
                    })??;
                Err(anyhow!("ICDI command response contained error {}", err).into())
            } else {
                Ok(()) // assume ok
            }
        }
    }
}

impl Debug for ReceiveBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "buffer:[")?;
        for &c in &self[..] {
            if c.is_ascii() && !c.is_ascii_control() {
                write!(f, "{}", c as char)?;
            } else {
                write!(f, ",{},", c)?;
            }
        }
        write!(f, "]")
    }
}

impl Deref for ReceiveBuffer {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.data[0..self.len]
    }
}
