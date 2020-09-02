use crate::cpu;
use crate::gpu::GPU;
use crate::gpu::VRAM_END;
use crate::gpu::VRAM_START;
use std::fs::File;
use std::io::Read;

const DIVIDER_REGISTER: usize = 0xFF04;
const TIMER_REG: usize = 0xFF05;
const TIMER_CONTROL: usize = 0xFF07;
const TIMER_MODULO: usize = 0xFF06;

pub trait Memory {
    fn read(&self, address: u16) -> u8;
    fn write(&mut self, address: u16, value: u8);
}

pub enum Select {
    Buttons,
    Directions,
}

#[allow(unused_imports)]
use wasm_bindgen::prelude::*;

// Global emu struct.
pub struct Bus {
    pub memory: [u8; 0x10000],
    pub bootrom: [u8; 0x100],
    pub in_bios: u8,
    pub int_enabled: u8,
    pub int_flags: u8,
    pub clock: usize,
    pub ime: u8,
    pub joypad_io: Select,
    pub gpu: GPU,
    pub rom_start_signal: bool,
}

fn load_bootrom() -> Vec<u8> {
    let mut file = File::open("dmg_boot.bin").expect("Couldn't open bootrom file.");
    let mut bootrom = Vec::new();
    file.read_to_end(&mut bootrom)
        .expect("Couldn't read the file.");
    bootrom
}

impl Bus {
    pub fn new(rom_vec: Vec<u8>) -> Self {
        let mut memory = [0; 0x10000];
        let mut bootrom = [0; 0x100];
        let bootrom_vec = load_bootrom();
        bootrom[..].clone_from_slice(&bootrom_vec[..]);
        memory[..rom_vec.len()].clone_from_slice(&rom_vec[..]);

        Bus {
            memory,
            bootrom,
            ime: 0,
            in_bios: 0,
            int_enabled: 0,
            int_flags: 0,
            clock: 0,
            joypad_io: Select::Buttons,
            gpu: GPU::new(),
            rom_start_signal: false,
        }
    }

    pub fn enable_interrupts(&mut self) {
        self.ime = 1;
    }

    pub fn disable_interrupts(&mut self) {
        self.ime = 0;
    }

    pub fn handle_interrupt(&mut self, flag: u8) {
        self.ime = 0;
        self.int_flags &= !flag;
    }

    fn tick_timer_counter(&mut self) {
        if self.clock % 256 == 0 {
            self.memory[DIVIDER_REGISTER] = self.memory[DIVIDER_REGISTER].wrapping_add(1);
        }
        let control = self.memory[TIMER_CONTROL];
        let clock_select = control & 0b11;
        let enable = (control & 0b100) != 0;
        let clock_speed = match clock_select {
            0b00 => 256,
            0b01 => 4,
            0b10 => 16,
            0b11 => 64,
            _ => unreachable!(),
        };
        if enable && self.clock % clock_speed == 0 {
            let (value, overflow) = self.memory[TIMER_REG].overflowing_add(1);
            if overflow {
                self.int_flags |= cpu::TIMER;
                self.memory[TIMER_REG] = self.memory[TIMER_MODULO]
            } else {
                self.memory[TIMER_REG] = value;
            }
        }
    }
    fn tick(&mut self) {
        self.clock += 1;
        self.tick_timer_counter();
    }

    pub fn cycle(&mut self) {
        self.gpu.cycle(&mut self.int_flags);
        self.tick();
    }
}

impl Memory for Bus {
    fn read(&self, address: u16) -> u8 {
        match address as usize {
            0x0000..=0x0100 if self.in_bios == 0 => self.bootrom[address as usize],
            0xFF40 => self.gpu.lcdc,
            0xFF41 => self.gpu.lcdstat,
            0xFF42 => self.gpu.vscroll,
            0xFF43 => self.gpu.hscroll,
            0xFF44 => self.gpu.scanline,
            0xFF47 => panic!("0xFF47 (bg_palette) is WRITE ONLY"),
            0xFF4A => self.gpu.windowy,
            0xFF4B => self.gpu.windowx,
            0xffff => self.int_enabled,
            0xff0f => self.int_flags,
            0xff00 => {
                match self.joypad_io {
                    Select::Buttons => {
                        return 0xff; //Todo
                    }
                    Select::Directions => {
                        return 0xff;
                    }
                }
            }
            // 0xFFFF => &self.gpu.,
            // 0xFF01 => {println!("R: ACC SERIAL TRANSFER DATA"); &self.memory[ias usize]},
            // 0xFF02 => {println!("R: ACC SERIAL TRANSFER DATA FLGS"); &self.memory[i as usize]},
            VRAM_START..=VRAM_END => self.gpu[address],
            _ => self.memory[address as usize],
        }
    }
    fn write(&mut self, address: u16, value: u8) {
        match address as usize {
            0x0000..=0x0100 if self.in_bios == 0 => panic!(),
            DIVIDER_REGISTER => self.memory[DIVIDER_REGISTER] = 0,
            0xff40 => self.gpu.lcdc = value,
            0xff41 => self.gpu.lcdstat = value,
            0xff42 => self.gpu.vscroll = value,
            0xff43 => self.gpu.hscroll = value,
            0xff44 => self.gpu.scanline = value,
            0xff47 => self.gpu.bg_palette = value,
            0xff4a => self.gpu.windowy = value,
            0xff4b => self.gpu.windowx = value,
            0xffff => self.int_enabled = value,
            0xff0f => {
                self.int_flags |= value;
            }
            0xff50 => {
                if value != 0 {
                    self.rom_start_signal = true;
                }
                self.in_bios = value
            }
            0xff80 => {
                self.memory[address as usize] = value;
            }
            0xff00 => {
                let select_buttons = value & 0b0010_0000 != 0;
                let select_directions = value & 0b0001_0000 != 0;
                if select_buttons {
                    self.joypad_io = Select::Buttons;
                } else if select_directions {
                    self.joypad_io = Select::Directions;
                }
            }
            0xff01 => {
                self.memory[address as usize] = value;
            }
            0xff02 => {
                if value == 0x81 {
                    // warn!("{}", char::from(self.memory[0xff01]));
                }
                self.memory[address as usize] = value;
            }
            VRAM_START..=VRAM_END => self.gpu.vram[address as usize - VRAM_START] = value,
            _ => self.memory[address as usize] = value,
        }
    }
}
