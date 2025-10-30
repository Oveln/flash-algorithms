#![no_std]
#![no_main]

use core::num::NonZeroU32;
use panic_halt as _;

use flash_algorithm::*;

// use ch32v3::ch32v30x as pac;
use ch32_metapac::{FLASH, flash::regs::{Addr, Keyr, Modekeyr}};

struct Algorithm;

const FLASH_KEY1: u32 = 0x45670123;
const FLASH_KEY2: u32 = 0xCDEF89AB;

const ERASE_TIMEOUT: u32 = 0xF00000;

algorithm!(Algorithm, {
    device_name: "ch32v208",
    device_type: DeviceType::Onchip,
    flash_address: 0x0000_0000,
    flash_size: 0x10000,
    page_size: 0x100,
    // Note: This is not correct, each erased word looks like: 0xe339e339
    empty_value: 0x39,
    program_time_out: 1000,
    erase_time_out: 2000,
    sectors: [{
        size: 0x8000,
        address: 0x0000000,
    }]
});

#[derive(Debug, Clone, Copy)]
pub enum Error {
    /// Generic error
    Generic = 1,
    /// Timeout error during write operation
    WriteTimeout = 2,
    /// Error during flash unlock or access
    UnlockError = 3,
    /// Invalid address alignment
    InvalidAddress = 4,
    /// Timeout error during erase operation
    EraseTimeout = 5,
    /// Flash is locked
    FlashLocked = 6,
    /// Programming error
    ProgrammingError = 7,
    /// Verification error
    VerificationError = 8,
    /// Unknown flash state error
    UnknownFlashState = 9,
    /// Busy error with additional timeout
    BusyTimeout = 10,
}

impl From<Error> for ErrorCode {
    fn from(value: Error) -> Self {
        unsafe {
            NonZeroU32::new_unchecked(value as u32)
        }
    }
}

fn wait_until_not_write_busy() -> Result<(), ErrorCode> {
    for _ in 0..ERASE_TIMEOUT {
        let status = FLASH.statr().read();
        if status.wr_bsy() {
            continue;
        }
        if status.wrprterr() {
            return Err(Error::ProgrammingError.into());
        }
        return Ok(());
    }
    Err(Error::WriteTimeout.into())
}

fn wait_until_not_busy() -> Result<(), ErrorCode> {
    for _ in 0..ERASE_TIMEOUT {
        let status = FLASH.statr().read();
        if status.bsy() && !status.eop() {
            continue;
        }
        if status.wrprterr() {
            return Err(Error::ProgrammingError.into());
        }
        FLASH.statr().modify(|w| {
            w.set_eop(false);
        });
        return Ok(());
    }
    Err(Error::EraseTimeout.into())
}

impl FlashAlgorithm for Algorithm {
    fn new(_address: u32, _clock: u32, _function: Function) -> Result<Self, ErrorCode> {
        // Unlock the flash
        FLASH.keyr().write_value(Keyr(FLASH_KEY1));
        FLASH.keyr().write_value(Keyr(FLASH_KEY2));

        // Unlock Quick Program Mode
        FLASH.modekeyr().write_value(Modekeyr(FLASH_KEY1));
        FLASH.modekeyr().write_value(Modekeyr(FLASH_KEY2));

        Ok(Self)
    }

    fn erase_sector(&mut self, addr: u32) -> Result<(), ErrorCode> {
        let addr = addr + 0x0800_0000;
        if addr & 0x7FFF != 0 {
            return Err(Error::InvalidAddress.into());
        }
        let addr = Addr(addr);
        wait_until_not_busy()?;

        FLASH.ctlr().modify(|w| w.set_ber32(true));
        FLASH.addr().write_value(addr);
        FLASH.ctlr().modify(|w| w.set_strt(true));
        wait_until_not_busy()?;
        FLASH.ctlr().modify(|w| w.set_ber32(false));
        Ok(())
    }

    fn program_page(&mut self, addr: u32, data: &[u8]) -> Result<(), ErrorCode> {
        let addr = addr + 0x0800_0000;
        let ctlr = FLASH.ctlr().read();
        if ctlr.lock() || ctlr.flock() {
            return Err(Error::FlashLocked.into());
        }
        if addr & 0xFF != 0 {
            return Err(Error::InvalidAddress.into());
        }
        let addr = Addr(addr);

        FLASH.ctlr().modify(|w| w.set_page_pg(true));
        wait_until_not_busy()?;

        for (word, addr) in data.chunks_exact(4).zip((addr.0..).step_by(4)) {
            let word = u32::from_le_bytes(word.try_into().unwrap());
            unsafe {
                (addr as *mut u32).write_volatile(word);
                wait_until_not_write_busy()?;
            };
        }

        FLASH.ctlr().modify(|w| w.set_pgstart(true));
        wait_until_not_busy()?;
        FLASH.ctlr().modify(|w| w.set_page_pg(false));

        Ok(())
    }
}

impl Drop for Algorithm {
    fn drop(&mut self) {
        // Lock the flash
        FLASH.ctlr().modify(|w| w.set_lock(true));
    }
}
