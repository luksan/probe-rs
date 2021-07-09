use std::cmp::min;
use std::convert::TryInto;
use std::io::Write;

use anyhow::{anyhow, Context};
use hex::FromHex;

use crate::architecture::arm::memory::adi_v5_memory_interface::Dhcsr;
use crate::core::CoreRegister;
use crate::probe::ti_icdi::receive_buffer::ReceiveBuffer;
use crate::probe::ti_icdi::usb_interface;
use crate::probe::ti_icdi::usb_interface::new_send_buffer;
use crate::DebugProbeError;

pub const ICDI_MAX_PACKET_SIZE: u32 = 2048;
pub const ICDI_MAX_RW_PACKET: u32 = (((ICDI_MAX_PACKET_SIZE - 64) / 4) * 4) / 2;

pub enum TargetRunState {
    Halted,
    Running,
}

pub trait HlaInterface {
    // fn open(&mut self) -> Result<(), DebugProbeError>;
    // fn close(&mut self) -> Result<(), DebugProbeError>;
    // fn idcode(&mut self) -> Result<(), DebugProbeError>;
    fn state(&mut self) -> Result<TargetRunState, DebugProbeError> {
        let mem = self.read_mem(Dhcsr::ADDRESS, 4, 1)?;
        let dhcsr: Dhcsr = u32::from_le_bytes(mem.as_slice().try_into().unwrap()).into();
        Ok(if dhcsr.s_halt() {
            TargetRunState::Halted
        } else {
            TargetRunState::Running
        })
    }
    fn reset(&mut self) -> Result<(), DebugProbeError>;
    // fn assert_srst(&mut self) -> Result<(), DebugProbeError>;
    fn run(&mut self) -> Result<(), DebugProbeError> {
        self.send_cmd(b"c")?
            .check_cmd_result()
            .context("Run command failed")
            .map_err(|e| e.into())
    }
    fn halt(&mut self) -> Result<(), DebugProbeError> {
        self.send_cmd(b"?")?
            .check_cmd_result()
            .context("Halt failed.")
            .map_err(|e| e.into())
    }
    fn step(&mut self) -> Result<(), DebugProbeError> {
        self.send_cmd(b"s")?
            .check_cmd_result()
            .context("Step command failed")
            .map_err(|e| e.into())
    }

    // fn read_regs(&mut self) -> Result<(), DebugProbeError>;
    fn read_reg(&mut self, regsel: u32) -> Result<u32, DebugProbeError> {
        let mut buf = Vec::with_capacity(10);
        write!(&mut buf, "p{:x}", regsel).unwrap();
        let buf = self.send_cmd(&buf)?;
        buf.check_cmd_result()?;
        let x = buf.get_payload()?;
        log::trace!("read reg response {:?}", x);
        let y = <[u8; 4]>::from_hex(x)
            .map_err(|_| DebugProbeError::Other(anyhow!("Hex conversion failed {:?}", buf)))?;

        Ok(u32::from_le_bytes(y))
    }
    fn write_reg(&mut self, regsel: u32, val: u32) -> Result<(), DebugProbeError> {
        let mut buf = Vec::with_capacity(20);
        write!(&mut buf, "P{:x}=", regsel).unwrap();
        usb_interface::write_hex(&mut buf, &val.to_le_bytes());
        self.send_cmd(&buf)?;
        Ok(())

        // FIXME: check response
    }
    fn read_mem(
        &mut self,
        mut addr: u32,
        size: u32,
        count: u32,
    ) -> Result<Vec<u8>, DebugProbeError> {
        let mut len = size.checked_mul(count).unwrap();
        let mut buf = Vec::with_capacity(len as usize);
        while len > 0 {
            let read_len = min(len, ICDI_MAX_RW_PACKET);
            self.read_mem_int(addr, read_len, &mut buf)?;
            addr += read_len;
            len -= read_len;
        }
        Ok(buf)
    }
    fn write_mem(&mut self, mut addr: u32, data: &[u8]) -> Result<(), DebugProbeError> {
        for chunk in data.chunks(ICDI_MAX_RW_PACKET as usize) {
            self.write_mem_int(addr, chunk)?;
            addr += chunk.len() as u32;
        }
        Ok(())
    }
    fn write_debug_reg(&mut self, addr: u32, val: u32) -> Result<(), DebugProbeError> {
        self.write_mem(addr, &val.to_le_bytes())
    }
    // fn override_target(&mut self);
    fn custom_command(&mut self, cmd: &[u8]) -> Result<ReceiveBuffer, DebugProbeError> {
        let mut buf = usb_interface::new_send_buffer(cmd.len() + 6);
        buf.extend_from_slice(b"qRcmd,");
        usb_interface::write_hex(&mut buf, cmd);
        self.send_packet(buf)
    }

    fn send_cmd(&mut self, cmd: &[u8]) -> Result<ReceiveBuffer, DebugProbeError> {
        let mut buf = new_send_buffer(cmd.len());
        buf.extend_from_slice(cmd);
        self.send_packet(buf)
    }

    fn read_mem_int(
        &mut self,
        addr: u32,
        len: u32,
        buf: &mut Vec<u8>,
    ) -> Result<(), DebugProbeError>;
    fn write_mem_int(&mut self, addr: u32, data: &[u8]) -> Result<(), DebugProbeError>;
    fn send_packet(&mut self, data: Vec<u8>) -> Result<ReceiveBuffer, DebugProbeError>;
}
