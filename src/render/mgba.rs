//! A GBA in libmgba, through the shim in mgba.c: one instruction at a time,
//! with the registers, memory, keys and picture the battle run needs.

use std::ffi::c_void;
use std::marker::PhantomData;
use std::ptr::NonNull;

pub const WIDTH: usize = 240;
pub const HEIGHT: usize = 160;

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct Step {
    /// The instruction the CPU runs next.
    pub pc: u32,
    /// Frames completed since power-on.
    pub frame: u32,
    /// The line being drawn.
    pub vcount: i32,
}

extern "C" {
    fn alchemy_mgba_open(rom: *const u8, size: usize) -> *mut c_void;
    fn alchemy_mgba_close(machine: *mut c_void);
    fn alchemy_mgba_step(machine: *mut c_void, out: *mut Step);
    fn alchemy_mgba_register(machine: *mut c_void, n: u32) -> u32;
    fn alchemy_mgba_set_register(machine: *mut c_void, n: u32, value: u32);
    fn alchemy_mgba_branch(machine: *mut c_void, address: u32);
    fn alchemy_mgba_read(machine: *mut c_void, address: u32, size: u32) -> u32;
    fn alchemy_mgba_write(machine: *mut c_void, address: u32, size: u32, value: u32);
    fn alchemy_mgba_set_keys(machine: *mut c_void, keys: u32);
    fn alchemy_mgba_pixels(machine: *const c_void) -> *const u32;
    fn alchemy_mgba_backdrop(machine: *mut c_void, lines: *const u16);
    fn alchemy_mgba_recolour(machine: *mut c_void, entries: *const u16, count: u32, table: *const u16);
    fn alchemy_mgba_state_size(machine: *mut c_void) -> usize;
    fn alchemy_mgba_layer(machine: *mut c_void, layer: u32, shown: bool);
    fn alchemy_mgba_bus_write16(machine: *mut c_void, address: u32, value: u16);
    fn alchemy_mgba_save_state(machine: *mut c_void, state: *mut u8) -> bool;
    fn alchemy_mgba_load_state(machine: *mut c_void, state: *const u8) -> bool;
}

pub const LR: u32 = 14;

/// The KEYINPUT bits.
pub const KEY_A: u32 = 1;
pub const KEY_RIGHT: u32 = 0x10;
pub const KEY_LEFT: u32 = 0x20;
pub const KEY_UP: u32 = 0x40;
pub const KEY_DOWN: u32 = 0x80;
pub const SP: u32 = 13;

/// A powered-on GBA running `rom`, which it borrows for its lifetime.
pub struct Gba<'rom> {
    machine: NonNull<c_void>,
    rom: PhantomData<&'rom [u8]>,
}

impl<'rom> Gba<'rom> {
    pub fn new(rom: &'rom [u8]) -> Result<Gba<'rom>, String> {
        let machine = unsafe { alchemy_mgba_open(rom.as_ptr(), rom.len()) };
        let machine = NonNull::new(machine).ok_or("libmgba cannot load the ROM")?;
        Ok(Gba { machine, rom: PhantomData })
    }

    /// Runs one instruction (or, while the CPU is halted, up to the next
    /// interrupt).
    pub fn step(&mut self) -> Step {
        let mut step = Step::default();
        unsafe { alchemy_mgba_step(self.machine.as_ptr(), &mut step) };
        step
    }

    pub fn register(&self, n: u32) -> u32 {
        unsafe { alchemy_mgba_register(self.machine.as_ptr(), n) }
    }

    pub fn set_register(&mut self, n: u32, value: u32) {
        unsafe { alchemy_mgba_set_register(self.machine.as_ptr(), n, value) }
    }

    /// Continues at a Thumb address.
    pub fn branch(&mut self, address: u32) {
        unsafe { alchemy_mgba_branch(self.machine.as_ptr(), address) }
    }

    /// Returns at once from the Thumb function about to start, with `r0`.
    pub fn return_now(&mut self, r0: u32) {
        self.set_register(0, r0);
        let lr = self.register(LR);
        self.branch(lr);
    }

    pub fn read8(&self, address: u32) -> u8 {
        unsafe { alchemy_mgba_read(self.machine.as_ptr(), address, 1) as u8 }
    }

    pub fn read16(&self, address: u32) -> u16 {
        unsafe { alchemy_mgba_read(self.machine.as_ptr(), address, 2) as u16 }
    }

    pub fn read32(&self, address: u32) -> u32 {
        unsafe { alchemy_mgba_read(self.machine.as_ptr(), address, 4) }
    }

    pub fn write8(&mut self, address: u32, value: u8) {
        unsafe { alchemy_mgba_write(self.machine.as_ptr(), address, 1, value.into()) }
    }

    pub fn write16(&mut self, address: u32, value: u16) {
        unsafe { alchemy_mgba_write(self.machine.as_ptr(), address, 2, value.into()) }
    }

    /// Stores a halfword through the bus, so an I/O register takes it.
    pub fn io_write16(&mut self, address: u32, value: u16) {
        unsafe { alchemy_mgba_bus_write16(self.machine.as_ptr(), address, value) }
    }

    pub fn write32(&mut self, address: u32, value: u32) {
        unsafe { alchemy_mgba_write(self.machine.as_ptr(), address, 4, value) }
    }

    pub fn set_keys(&mut self, keys: u32) {
        unsafe { alchemy_mgba_set_keys(self.machine.as_ptr(), keys) }
    }

    /// The last picture drawn, as BGR555 colours.
    pub fn picture(&self) -> Vec<u16> {
        let pixels = unsafe { std::slice::from_raw_parts(alchemy_mgba_pixels(self.machine.as_ptr()), WIDTH * HEIGHT) };
        // Each channel's top five bits: libmgba widens five bits to eight.
        let five = |c: u32, shift: u32| ((c >> shift) & 0xff) >> 3;
        pixels.iter().map(|&c| (five(c, 0) | five(c, 8) << 5 | five(c, 16) << 10) as u16).collect()
    }

    /// Draws each line from now on over a backdrop of our choosing, in BGR555;
    /// the game's palette RAM keeps its own colour 0.
    pub fn set_backdrop(&mut self, lines: &[u16; HEIGHT]) {
        unsafe { alchemy_mgba_backdrop(self.machine.as_ptr(), lines.as_ptr()) }
    }

    /// Draws each line with the game's own colour 0 again.
    pub fn clear_backdrop(&mut self) {
        unsafe { alchemy_mgba_backdrop(self.machine.as_ptr(), std::ptr::null()) }
    }

    /// Draws each of `entries` (palette entries, 0 to 255 BG and 256 to 511 OBJ)
    /// from now on as `table` maps its colour in palette RAM; the RAM keeps
    /// the game's colours.
    pub fn set_recolour(&mut self, entries: &[u16], table: &[u16]) {
        assert_eq!(table.len(), 0x8000);
        unsafe { alchemy_mgba_recolour(self.machine.as_ptr(), entries.as_ptr(), entries.len() as u32, table.as_ptr()) }
    }
}

impl Gba<'_> {
    /// The machine's whole state, to load again later.
    pub fn save(&mut self) -> Vec<u8> {
        let size = unsafe { alchemy_mgba_state_size(self.machine.as_ptr()) };
        let mut state = vec![0; size];
        let saved = unsafe { alchemy_mgba_save_state(self.machine.as_ptr(), state.as_mut_ptr()) };
        assert!(saved, "libmgba could not save the machine");
        state
    }

    /// Returns the machine to a state `save` took.
    pub fn load(&mut self, state: &[u8]) {
        let loaded = unsafe { alchemy_mgba_load_state(self.machine.as_ptr(), state.as_ptr()) };
        assert!(loaded, "libmgba could not load the machine");
    }

    /// Shows or hides a layer in the picture (0 to 3 the backgrounds, 4 the
    /// sprites); the game's registers are unchanged.
    pub fn show_layer(&mut self, layer: u32, shown: bool) {
        unsafe { alchemy_mgba_layer(self.machine.as_ptr(), layer, shown) }
    }

    /// The last picture drawn, as 0xRRGGBB.
    pub fn picture_rgb(&self) -> Vec<u32> {
        let pixels = unsafe { std::slice::from_raw_parts(alchemy_mgba_pixels(self.machine.as_ptr()), WIDTH * HEIGHT) };
        pixels.iter().map(|&c| (c & 0xff) << 16 | (c & 0xff00) | (c >> 16) & 0xff).collect()
    }
}

impl Drop for Gba<'_> {
    fn drop(&mut self) {
        unsafe { alchemy_mgba_close(self.machine.as_ptr()) }
    }
}
