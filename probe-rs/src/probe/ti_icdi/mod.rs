mod gdb_interface;
mod receive_buffer;
mod usb_interface;

use crate::{
    CoreRegisterAddress, DebugProbe, DebugProbeError, DebugProbeSelector, Error, Memory, Probe,
    WireProtocol,
};

use crate::architecture::arm::ap::{AccessPort, GenericAp, MemoryAp};
use crate::architecture::arm::dp::DebugPortVersion;
use crate::architecture::arm::memory::Component;
use crate::architecture::arm::{
    ApInformation, ArmChipInfo, ArmProbeInterface, DapAccess, MemoryApInformation, SwoAccess,
    SwoConfig,
};
use std::time::Duration;
pub use usb_interface::list_icdi_devices;
use usb_interface::IcdiUsbInterface;

use crate::architecture::arm::memory::adi_v5_memory_interface::ArmProbe;
use crate::probe::ti_icdi::gdb_interface::GdbRemoteInterface;
use crate::Error as ProbeRsError;
use std::convert::TryInto;

#[derive(Debug)]
pub struct IcdiProbe {
    device: IcdiUsbInterface,
    protocol: WireProtocol,
    name: String,
}

impl IcdiProbe {
    pub fn get_memory(&mut self) -> Memory<'_> {
        Memory::new(self, MemoryAp::new(0))
    }
}

impl DebugProbe for IcdiProbe {
    fn new_from_selector(
        selector: impl Into<DebugProbeSelector>,
    ) -> Result<Box<Self>, DebugProbeError>
    where
        Self: Sized,
    {
        let mut device = IcdiUsbInterface::new_from_selector(selector)?;
        let ver = device.query_icdi_version()?;
        let name = format!("ICDI S/N: {}, ver: {}", &device.serial_number, ver);
        Ok(Box::new(Self {
            device,
            protocol: WireProtocol::Jtag,
            name,
        }))
    }

    fn get_name(&self) -> &str {
        &self.name
    }

    fn speed(&self) -> u32 {
        1120 // FIXME!!
    }

    fn set_speed(&mut self, speed_khz: u32) -> Result<u32, DebugProbeError> {
        Err(DebugProbeError::UnsupportedSpeed(speed_khz))
    }

    fn attach(&mut self) -> Result<(), DebugProbeError> {
        log::debug!("attach({:?})", self.protocol);
        self.device
            .send_cmd(b"qSupported")
            .and_then(|r| r.check_cmd_result())?;
        self.device
            .send_cmd(b"!")
            .and_then(|r| r.check_cmd_result())
    }

    fn detach(&mut self) -> Result<(), DebugProbeError> {
        log::debug!("Detaching from TI-ICDI.");
        self.device
            .send_remote_command(b"debug disable")
            .and_then(|r| r.check_cmd_result())
    }

    fn target_reset(&mut self) -> Result<(), DebugProbeError> {
        self.device
            .send_remote_command(b"debug hreset")?
            .check_cmd_result()
    }

    fn target_reset_assert(&mut self) -> Result<(), DebugProbeError> {
        self.device
            .send_remote_command(b"debug sreset")
            .and_then(|r| r.check_cmd_result())
    }

    fn target_reset_deassert(&mut self) -> Result<(), DebugProbeError> {
        self.device
            .send_remote_command(b"debug hreset")
            .and_then(|r| r.check_cmd_result())
    }

    fn select_protocol(&mut self, protocol: WireProtocol) -> Result<(), DebugProbeError> {
        match protocol {
            WireProtocol::Jtag => {
                self.protocol = protocol;
                Ok(())
            }
            _ => Err(DebugProbeError::UnsupportedProtocol(protocol)),
        }
    }

    fn has_arm_interface(&self) -> bool {
        true
    }
    fn try_get_arm_interface<'probe>(
        self: Box<Self>,
    ) -> Result<Box<dyn ArmProbeInterface + 'probe>, (Box<dyn DebugProbe>, DebugProbeError)> {
        Ok(self)
    }

    fn into_probe(self: Box<Self>) -> Box<dyn DebugProbe> {
        self
    }
}

impl ArmProbeInterface for IcdiProbe {
    fn memory_interface(&mut self, access_port: MemoryAp) -> Result<Memory<'_>, Error> {
        Ok(Memory::new(self, access_port))
    }

    fn ap_information(&self, access_port: GenericAp) -> Option<&ApInformation> {
        if access_port.port_number() != 0 {
            return None;
        }
        Some(&ApInformation::MemoryAp(MemoryApInformation {
            port_number: 0,
            only_32bit_data_size: false,
            debug_base_address: 0xE00FF000, // FIXME: This might only be true for Cortex-M4
            supports_hnonsec: false,
        }))
    }

    fn num_access_ports(&self) -> usize {
        1
    }

    fn read_from_rom_table(&mut self) -> Result<Option<ArmChipInfo>, Error> {
        todo!();
        let baseaddr = 0xE00FF000; // FIXME: This might only be true for Cortex-M4
        let mut memory = self.get_memory();
        let component = Component::try_parse(&mut memory, baseaddr)
            .map_err(ProbeRsError::architecture_specific)?;

        if let Component::Class1RomTable(component_id, _) = component {
            if let Some(jep106) = component_id.peripheral_id().jep106() {
                return Ok(Some(ArmChipInfo {
                    manufacturer: jep106,
                    part: component_id.peripheral_id().part(),
                }));
            }
        }
        Ok(None)
    }

    fn target_reset_deassert(&mut self) -> Result<(), Error> {
        self.device
            .send_remote_command(b"debug hreset")
            .and_then(|response| response.check_cmd_result())
            .map_err(Error::Probe)
    }

    fn close(self: Box<Self>) -> Probe {
        Probe::new(*self)
    }
}

impl DapAccess for IcdiProbe {
    fn debug_port_version(&self) -> DebugPortVersion {
        DebugPortVersion::Unsupported(255)
    }

    fn read_raw_dp_register(&mut self, _addr: u8) -> Result<u32, DebugProbeError> {
        Err(DebugProbeError::CommandNotSupportedByProbe)
    }

    fn write_raw_dp_register(&mut self, _addr: u8, _value: u32) -> Result<(), DebugProbeError> {
        Err(DebugProbeError::CommandNotSupportedByProbe)
    }

    fn read_raw_ap_register(&mut self, _port: u8, _addr: u8) -> Result<u32, DebugProbeError> {
        Err(DebugProbeError::CommandNotSupportedByProbe)
    }

    fn write_raw_ap_register(
        &mut self,
        _port: u8,
        _addr: u8,
        _value: u32,
    ) -> Result<(), DebugProbeError> {
        Err(DebugProbeError::CommandNotSupportedByProbe)
    }
}

impl SwoAccess for IcdiProbe {
    fn enable_swo(&mut self, _config: &SwoConfig) -> Result<(), Error> {
        Err(Error::Probe(DebugProbeError::CommandNotSupportedByProbe))
    }

    fn disable_swo(&mut self) -> Result<(), Error> {
        Err(Error::Probe(DebugProbeError::CommandNotSupportedByProbe))
    }

    fn read_swo_timeout(&mut self, _timeout: Duration) -> Result<Vec<u8>, Error> {
        Err(Error::Probe(DebugProbeError::CommandNotSupportedByProbe))
    }
}

impl ArmProbe for &mut IcdiProbe {
    fn read_core_reg(&mut self, _ap: MemoryAp, addr: CoreRegisterAddress) -> Result<u32, Error> {
        log::trace!("Read core reg {}", addr.0);
        self.device.read_reg(addr.0 as u32).map_err(Error::Probe)
    }

    fn write_core_reg(
        &mut self,
        _ap: MemoryAp,
        addr: CoreRegisterAddress,
        value: u32,
    ) -> Result<(), Error> {
        log::trace!("Write core reg {} {}", addr.0, value);
        self.device
            .write_reg(addr.0 as u32, value)
            .map_err(Error::Probe)
    }

    fn read_8(&mut self, _ap: MemoryAp, address: u32, data: &mut [u8]) -> Result<(), Error> {
        self.device.read_mem(address, data).map_err(Error::Probe)
    }

    fn read_32(&mut self, _ap: MemoryAp, address: u32, data: &mut [u32]) -> Result<(), Error> {
        let u32len = data.len();
        log::trace!("read_32 address {:08x}, len {:x}", address, u32len);
        log::trace!("read_32 pre {:?}", data);
        // Safety: Four u8 to every u32, all values valid
        let mut as_u8 = vec![0u8; u32len * 4];
        //        let (_, as_u8, _) = unsafe { data.align_to_mut::<u8>() };
        assert_eq!(as_u8.len(), u32len * 4);
        self.device.read_mem(address, as_u8.as_mut_slice())?;
        for (d, c) in data.iter_mut().zip(as_u8.chunks_exact(4)) {
            *d = u32::from_le_bytes(c.try_into().unwrap());
        }
        log::trace!(
            "read_32 result {:08x}, {:?}",
            data[0],
            data[0].to_ne_bytes()
        );
        return Ok(());
        for uint in as_u8.chunks_exact_mut(4) {
            uint.copy_from_slice(
                &u32::from_be_bytes((uint as &[u8]).try_into().unwrap()).to_ne_bytes()[..],
            );
        }
        Ok(())
    }

    fn write_8(&mut self, _ap: MemoryAp, address: u32, data: &[u8]) -> Result<(), Error> {
        self.device.write_mem(address, data)?;
        Ok(())
    }

    fn write_32(&mut self, _ap: MemoryAp, address: u32, data: &[u32]) -> Result<(), Error> {
        let mut bu8 = Vec::with_capacity(data.len() * 4);
        for d in data {
            bu8.extend_from_slice(&d.to_le_bytes()[..]);
        }
        self.device.write_mem(address, bu8.as_slice())?;
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Error> {
        Ok(())
    }
}
