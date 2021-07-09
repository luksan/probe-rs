#![allow(unused_variables, unused_imports)]

mod hla_interface;
mod receive_buffer;
mod usb_interface;

use anyhow::anyhow;

use crate::{DebugProbe, DebugProbeError, DebugProbeSelector, Error, Memory, Probe, WireProtocol};

use crate::architecture::arm::ap::{
    valid_access_ports, ApAccess, ApClass, GenericAp, MemoryAp, IDR,
};
use crate::architecture::arm::communication_interface::ArmCommunicationInterfaceState;
use crate::architecture::arm::dp::DebugPortVersion;
use crate::architecture::arm::memory::Component;
use crate::architecture::arm::{
    ApInformation, ArmChipInfo, ArmProbeInterface, DapAccess, SwoAccess, SwoConfig,
};
use std::time::Duration;
pub use usb_interface::list_icdi_devices;
use usb_interface::{IcdiUsbInterface, TIMEOUT};

use crate::probe::ti_icdi::hla_interface::HlaInterface;
use crate::Error as ProbeRsError;

const DP_PORT: u16 = 0xFFFF;

#[derive(Debug)]
pub(crate) struct IcdiProbe {
    device: IcdiUsbInterface,
    protocol: WireProtocol,
}

impl IcdiProbe {}

impl DebugProbe for IcdiProbe {
    fn new_from_selector(
        selector: impl Into<DebugProbeSelector>,
    ) -> Result<Box<Self>, DebugProbeError>
    where
        Self: Sized,
    {
        let usb = IcdiUsbInterface::new_from_selector(selector)?;
        Ok(Box::new(Self {
            device: usb,
            protocol: WireProtocol::Jtag,
        }))
    }

    fn get_name(&self) -> &str {
        "ICDI name <TODO>"
    }

    fn speed(&self) -> u32 {
        1120 // FIXME!!
    }

    fn set_speed(&mut self, speed_khz: u32) -> Result<u32, DebugProbeError> {
        Err(DebugProbeError::UnsupportedSpeed(speed_khz))
    }

    fn attach(&mut self) -> Result<(), DebugProbeError> {
        log::debug!("attach({:?})", self.protocol);
        self.device.send_cmd(b"qSupported")?;

        Ok(())
    }

    fn detach(&mut self) -> Result<(), DebugProbeError> {
        log::info!("Detaching from TI-ICDI.");

        Ok(())
    }

    fn target_reset(&mut self) -> Result<(), DebugProbeError> {
        todo!()
    }

    fn target_reset_assert(&mut self) -> Result<(), DebugProbeError> {
        todo!()
    }

    fn target_reset_deassert(&mut self) -> Result<(), DebugProbeError> {
        todo!()
    }

    fn select_protocol(&mut self, protocol: WireProtocol) -> Result<(), DebugProbeError> {
        match protocol {
            WireProtocol::Jtag | WireProtocol::Swd => {
                self.protocol = protocol;
                Ok(())
            }
            #[allow(unreachable_patterns)]
            _ => Err(DebugProbeError::UnsupportedProtocol(protocol)),
        }
    }

    fn has_arm_interface(&self) -> bool {
        false
    }

    fn into_probe(self: Box<Self>) -> Box<dyn DebugProbe> {
        self
    }
}
