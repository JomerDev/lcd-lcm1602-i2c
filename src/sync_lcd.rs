use embedded_hal::delay::DelayNs;
use embedded_hal::i2c::I2c;

use ufmt_write::uWrite;

use crate::{
    Backlight, BitMode, Commands, CursorMoveDir, DisplayControl, DisplayShift, Font, Mode, OFFSETS_16X4, OFFSETS_NORMAL,
};

/// API to write to the LCD.
pub struct Lcd<'a, const ROWS: u8, const COLUMNS: u8, I, D>
where
    I: I2c,
    D: DelayNs,
{
    i2c: &'a mut I,
    address: u8,
    delay: &'a mut D,
    backlight_state: Backlight,
    cursor_on: bool,
    cursor_blink: bool,
    font_mode: Font,
}

impl<'a, const ROWS: u8, const COLUMNS: u8, I, D> Lcd<'a, ROWS, COLUMNS, I, D>
where
    I: I2c,
    D: DelayNs,
{
    /// Create new instance with only the I2C and delay instance.
    pub fn new(i2c: &'a mut I, delay: &'a mut D) -> Self {
        assert!(ROWS > 0, "ROWS needs to be larger than zero!");
        assert!(COLUMNS > 0, "COLUMNS needs to be larger than zero!");
        assert!(ROWS < 5, "This library only supports LCDs with up to four rows!"); // Because we don't have offets for more than four rows
        Self {
            i2c,
            delay,
            backlight_state: Backlight::On,
            address: 0,
            cursor_blink: false,
            cursor_on: false,
            font_mode: Font::Font5x8,
        }
    }

    /// Set I2C address, see [lcd address].
    ///
    /// [lcd address]: https://badboi.dev/rust,/microcontrollers/2020/11/09/i2c-hello-world.html
    pub fn with_address(mut self, address: u8) -> Self {
        self.address = address;
        self
    }

    pub fn with_cursor_on(mut self, on: bool) -> Self {
        self.cursor_on = on;
        self
    }

    pub fn with_cursor_blink(mut self, blink: bool) -> Self {
        self.cursor_blink = blink;
        self
    }

    /// Initializes the hardware.
    ///
    /// Actual procedure is a bit obscure. This one was compiled from this [blog post],
    /// corresponding [code] and the [datasheet].
    ///
    /// [datasheet]: https://www.openhacks.com/uploadsproductos/eone-1602a1.pdf
    /// [code]: https://github.com/jalhadi/i2c-hello-world/blob/main/src/main.rs
    /// [blog post]: https://badboi.dev/rust,/microcontrollers/2020/11/09/i2c-hello-world.html
    pub fn init(mut self) -> Result<Self, I::Error> {
        // Initial delay to wait for init after power on.
        self.delay.delay_ms(80);

        self.backlight(self.backlight_state)?;

        self.delay.delay_ms(1);

        // Init with 8 bit mode
        let mode_8bit = Mode::FunctionSet as u8 | BitMode::Bit8 as u8;
        self.write4bits(mode_8bit)?;
        self.delay.delay_ms(5);
        self.write4bits(mode_8bit)?;
        self.delay.delay_ms(5);
        self.write4bits(mode_8bit)?;
        self.delay.delay_ms(5);

        // Switch to 4 bit mode
        let mode_4bit = Mode::FunctionSet as u8 | BitMode::Bit4 as u8;
        self.write4bits(mode_4bit)?;

        self.update_function_set()?;

        self.update_display_control()?;
        self.command(Mode::Cmd as u8 | Commands::Clear as u8)?; // Clear Display

        self.delay.delay_ms(2);

        // Entry right: shifting cursor moves to right
        self.command(
            Mode::EntrySet as u8 | CursorMoveDir::Left as u8 | DisplayShift::Decrement as u8,
        )?;
        self.return_home()?;
        Ok(self)
    }

    fn write4bits(&mut self, data: u8) -> Result<(), I::Error> {
        self.i2c.write(
            self.address,
            &[data | DisplayControl::Off as u8 | self.backlight_state as u8],
        )?;
        self.i2c.write(
            self.address,
            &[data | DisplayControl::DisplayOn as u8 | self.backlight_state as u8],
        )?;
        self.i2c.write(
            self.address,
            &[DisplayControl::Off as u8 | self.backlight_state as u8],
        )?;
        self.delay.delay_us(700);
        Ok(())
    }

    fn send(&mut self, data: u8, mode: Mode) -> Result<(), I::Error> {
        let high_bits: u8 = data & 0xf0;
        let low_bits: u8 = (data << 4) & 0xf0;
        self.write4bits(high_bits | mode as u8)?;
        self.write4bits(low_bits | mode as u8)?;
        Ok(())
    }

    fn command(&mut self, data: u8) -> Result<(), I::Error> {
        self.send(data, Mode::Cmd)
    }

    pub fn backlight(&mut self, backlight: Backlight) -> Result<(), I::Error> {
        self.backlight_state = backlight;
        self.i2c
            .write(self.address, &[DisplayControl::Off as u8 | backlight as u8])
    }

    /// Write string to display.
    pub fn write_str(&mut self, data: &str) -> Result<(), I::Error> {
        for c in data.chars() {
            self.send(c as u8, Mode::Data)?;
        }
        Ok(())
    }

    /// Clear the display
    pub fn clear(&mut self) -> Result<(), I::Error> {
        self.command(Commands::Clear as u8)?;
        self.delay.delay_ms(2);
        Ok(())
    }

    /// Return cursor to upper left corner, i.e. (0,0).
    pub fn return_home(&mut self) -> Result<(), I::Error> {
        self.command(Commands::ReturnHome as u8)?;
        self.delay.delay_ms(2);
        Ok(())
    }

    /// Set the cursor to (rows, col). Coordinates are zero-based.
    pub fn set_cursor(&mut self, row: u8, col: u8) -> Result<(), I::Error> {
        assert!(row < ROWS, "Row needs to be smaller than ROWS");
        assert!(col < COLUMNS, "col needs to be smaller than COLUMNS");
        
        let offset = if ROWS == 4 && COLUMNS == 16 {
            OFFSETS_16X4[row as usize]
        } else {
            OFFSETS_NORMAL[row as usize]
        };

        let shift: u8 = col + offset;
        self.command(Mode::DDRAMAddr as u8 | shift)
    }

    /// Recomputes display_ctrl and updates the lcd
    fn update_display_control(&mut self) -> Result<(), I::Error> {
        let display_ctrl = if self.cursor_on {
            DisplayControl::DisplayOn as u8 | DisplayControl::CursorOn as u8
        } else {
            DisplayControl::DisplayOn as u8
        };
        let display_ctrl = if self.cursor_blink {
            display_ctrl | DisplayControl::CursorBlink as u8
        } else {
            display_ctrl
        };
        self.command(Mode::DisplayControl as u8 | display_ctrl)
    }

    // Set if the cursor is blinking
    pub fn cursor_blink(&mut self, blink: bool) -> Result<(), I::Error> {
        self.cursor_blink = blink;
        self.update_display_control()
    }

    // Set the curser visibility
    pub fn cursor_on(&mut self, on: bool) -> Result<(), I::Error> {
        self.cursor_on = on;
        self.update_display_control()
    }

    /// Recomputes function set and updates the lcd
    fn update_function_set(&mut self) -> Result<(), I::Error> {
        // Function set command
        let lines = match ROWS {
            1 => 0x00,
            _ => 0x08
        };
        self.command(
            Mode::FunctionSet as u8 | self.font_mode as u8 | lines, // Two line display
        )
    }

    /// Set the font mode used (5x8 or 5x10)
    pub fn font_mode(&mut self, mode: Font) -> Result<(), I::Error> {
        self.font_mode = mode;
        self.update_function_set()
    }

    /// Scrolls the display one char to the left
    pub fn scroll_display_left(&mut self) -> Result<(), I::Error> {
        self.command(Commands::ShiftDisplayLeft as u8)
    }

    /// Scrolls the display one char to the right
    pub fn scroll_display_right(&mut self) -> Result<(), I::Error> {
        self.command(Commands::ShiftDisplayRight as u8)
    }

    /// Scrolls the cursor one char to the left
    pub fn scroll_cursor_left(&mut self) -> Result<(), I::Error> {
        self.command(Commands::ShiftCursorLeft as u8)
    }

    /// Scrolls the cursor one char to the right
    pub fn scroll_cursor_right(&mut self) -> Result<(), I::Error> {
        self.command(Commands::ShiftCursorRight as u8)
    }

    /// Creates a new char in the specified memory location. If a char already exists there it will be overwritten
    pub fn create_char(&mut self, location: u8, charmap: &[u8]) -> Result<(), I::Error> {
        let location = location & 0x7;
        self.command(Mode::CGRAMAddr as u8 | location << 3)?;

        for i in 0..8 {
            self.send(charmap[i], Mode::Data)?;
        }
        Ok(())
    }
}

impl<'a, const ROWS: u8, const COLUMNS: u8, I, D> uWrite for Lcd<'a, ROWS, COLUMNS, I, D>
where
    I: I2c,
    D: DelayNs,
{
    type Error = I::Error;

    fn write_str(&mut self, s: &str) -> Result<(), Self::Error> {
        self.write_str(s)
    }
}
