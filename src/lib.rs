use std::time::{Duration, Instant};

use hidapi::{HidApi, HidDevice, HidError};

/// The keyboard to connect to, and the main struct of this library.
///
/// Call [`Keyboard::connect_to_any`] to connect to the first found ZSA device.
///
/// This is [`Send`] but not [`Sync`] because [`HidDevice`] is the same. To
/// make this both, you should spawn this on a separate thread and use channels
/// to communicate between.
#[derive(Debug)]
pub struct Keyboard {
    pub device: HidDevice,
    pub keyboard: KeyboardType,
    pub pid: u16,
    pub paired: bool,
    pub firmware: Option<String>,
    pub protocol: Option<u8>,
}
impl Keyboard {
    /// Connect to any currently-available ZSA keyboard, pairing it so it can
    /// also receive requests.
    ///
    /// # Errors
    ///
    /// This function will return an error if the device can not be written to.
    pub fn connect_to_any() -> Result<Option<Self>, PairingError> {
        let api = HidApi::new()?;
        let Some(mut kb) = Self::find_any(&api)? else {
            return Ok(None);
        };

        kb.get_firmware_version()?;
        kb.pair()?;
        kb.get_protocol()?;

        Ok(Some(kb))
    }

    /// Finds the first available ZSA keyboard and connects to it.
    ///
    /// # Errors
    ///
    /// This function will return an error if the keyboard could not be connected to.
    pub fn find_any(api: &HidApi) -> Result<Option<Self>, HidError> {
        for dev_info in api.device_list() {
            let vid = dev_info.vendor_id();
            let pid = dev_info.product_id();
            if dev_info.interface_number() != 1 {
                continue;
            }

            if let Some(keyboard) = KeyboardType::identify_keyboard(vid, pid) {
                let device = api.open_path(dev_info.path())?;
                let kb = Self {
                    device,
                    keyboard,
                    pid,
                    paired: false,
                    firmware: None,
                    protocol: None,
                };
                return Ok(Some(kb));
            }
        }
        Ok(None)
    }

    /// Pair the caller to the keyboard.
    ///
    /// # Errors
    ///
    /// This function will return an error if the keyboard denies the pairing,
    /// if it doesn't respond with a success within 200ms, or if there was an
    /// error reading from the device.
    pub fn pair(&mut self) -> Result<(), PairingError> {
        if self.paired {
            return Ok(());
        }

        self.request_pairing()?;
        let kb_response = self.read_until(
            |s, e| match e {
                KeyboardResponse::PairingFail(_) => true,
                _ => s.paired,
            },
            Duration::from_millis(200),
        )?;
        match kb_response {
            KeyboardResponse::PairingFail(bytes) => Err(PairingError::KeyboardDenied(bytes)),
            _ => Ok(()),
        }
    }

    /// Returns the firmware version of this device.
    ///
    /// # Errors
    ///
    /// This function will return an error if the device didn't respond with a
    /// firmware version within 200ms, or if there was an error reading from the
    /// device.
    #[allow(clippy::missing_panics_doc, reason = "it doesnt actually panic")]
    pub fn get_firmware_version(&mut self) -> Result<&str, ReadError> {
        if self.firmware.is_some() {
            // this is to circumvent some lifetime shenanigans
            #[allow(clippy::unwrap_used, reason = "we just checked .is_some()")]
            Ok(self.firmware.as_deref().unwrap())
        } else {
            self.request_firmware_version()?;
            self.read_until(|s, _| s.firmware.is_some(), Duration::from_millis(200))?;
            self.firmware.as_deref().ok_or(ReadError::TimeoutExceeded)
        }
    }

    /// Returns the firmware version of this device.
    ///
    /// # Errors
    ///
    /// This function will return an error if the device didn't respond with a
    /// firmware version within 200ms, or if there was an error reading from the
    /// device.
    pub fn get_protocol(&mut self) -> Result<u8, ReadError> {
        if let Some(protocol) = self.protocol {
            return Ok(protocol);
        }

        self.request_firmware_protocol_version()?;
        self.read_until(|s, _| s.protocol.is_some(), Duration::from_millis(200))?;
        self.protocol.ok_or(ReadError::TimeoutExceeded)
    }

    /// Blocks the current thread, reading values until `f` returns `true`.
    ///
    /// # Errors
    ///
    /// This function will return an error if the timeout expires or the device
    /// cannot be read.
    ///
    /// # Panics
    ///
    /// If the timeout is too long, this function will panic. This should not
    /// reasonably happen, as it requires overflowing an [`Instant`]
    pub fn read_until(
        &mut self,
        mut f: impl FnMut(&mut Self, &KeyboardResponse) -> bool,
        timeout: Duration,
    ) -> Result<KeyboardResponse, ReadError> {
        #[allow(
            clippy::arithmetic_side_effects,
            reason = "no reasonable person should get this"
        )]
        let end = Instant::now() + timeout;

        loop {
            let timeout = end.duration_since(Instant::now());
            #[allow(clippy::expect_used, reason = "no reasonable person should get this")]
            let millis = timeout.as_millis().try_into().expect("timeout is too long");
            let timeout = Duration::from_millis(millis);

            let Some(read) = self.read_timeout(timeout)? else {
                return Err(ReadError::TimeoutExceeded);
            };
            if f(self, &read) {
                return Ok(read);
            }
        }
    }

    /// Pair the computer with the keyboard, allowing it to take commands.
    /// This is required after plugging it in, otherwise commands will be
    /// silently ignored.
    ///
    /// # Errors
    ///
    /// This function will return an error if the device can not be written to.
    pub fn request_pairing(&self) -> Result<(), HidError> {
        self.write(Command::RequestPairing)
    }

    /// Requests the device to send across its firmware version.
    ///
    /// # Errors
    ///
    /// This function will return an error if the device can not be written to.
    pub fn request_firmware_version(&self) -> Result<(), HidError> {
        self.write(Command::RequestFirmwareVersion)
    }

    /// Requests the device to send across its protocol version.
    ///
    /// # Errors
    ///
    /// This function will return an error if the device can not be written to.
    pub fn request_firmware_protocol_version(&self) -> Result<(), HidError> {
        self.write(Command::RequestFirmwareProtocolVersion)
    }

    /// Sets the layer on the device. This locks it onto that specific layer
    /// similar to the "Direct switch to layer" option in Oryx.
    ///
    /// # Errors
    ///
    /// This function will return an error if the device can not be written to.
    ///
    /// See also: [`unset_layer`](Keyboard::unset_layer)
    pub fn set_layer(&self, layer: u8) -> Result<(), HidError> {
        self.write(Command::SetLayer {
            layer,
            activate_layer: true,
        })
    }

    /// Unsets the layer on the device. This locks it onto that specific layer
    /// similar to the "Direct switch to layer" option in Oryx.
    ///
    /// # Errors
    ///
    /// This function will return an error if the device can not be written to.
    ///
    /// See also: [`set_layer`](Keyboard::set_layer)
    pub fn unset_layer(&self, layer: u8) -> Result<(), HidError> {
        self.write(Command::SetLayer {
            layer,
            activate_layer: false,
        })
    }

    /// Sets a specific LED behind a key to a color.
    ///
    /// `key_index` is keyboard-dependent. As-is, there is no mapping of a key
    /// to its index. If you want to make one, go ahead!
    ///
    /// Also takes control of the keyboard's RGB. Call [`take_rgb_control(false)`]
    /// to reset the keyboard back to how the user defined.
    ///
    /// # Errors
    ///
    /// This function will return an error if the device can not be written to.
    ///
    /// See also: [`set_rgb_all`](Keyboard::set_rgb_all)
    ///
    /// [`take_rgb_control(false)`]: Self::take_rgb_control
    pub fn set_rgb_led(&self, key_index: u8, r: u8, g: u8, b: u8) -> Result<(), HidError> {
        self.write(Command::SetRgbLed { key_index, r, g, b })
    }

    /// Sets the LEDs behind every key to one color.
    ///
    /// Also takes control of the keyboard's RGB. Call [`take_rgb_control(false)`]
    /// to reset the keyboard back to how the user defined.
    ///
    /// # Errors
    ///
    /// This function will return an error if the device can not be written to.
    ///
    /// See also: [`set_rgb_led`](Keyboard::set_rgb_led)
    ///
    /// [`take_rgb_control(false)`]: Self::take_rgb_control
    pub fn set_rgb_all(&self, r: u8, g: u8, b: u8) -> Result<(), HidError> {
        self.write(Command::SetRgbAll { r, g, b })
    }

    /// Take control of the keyboard LEDs for yourself. This prevents the
    /// keyboard from setting the LEDs itself, for example with some effects.
    ///
    /// This is automatically done for you by the keyboard upon calling
    /// [`set_rgb_led`](Keyboard::set_rgb_led) or [`set_rgb_all`](Keyboard::set_rgb_all),
    /// meaning you generally only need to call this with `false`
    ///
    /// # Errors
    ///
    /// This function will return an error if the device can not be written to.
    pub fn take_rgb_control(&self, b: bool) -> Result<(), HidError> {
        self.write(Command::TakeRgbControl { take_control: b })
    }

    /// Enables or disables a status LED
    ///
    /// Also takes control of the keyboard's status LEDs.
    /// Call [`take_led_control(false)`] to reset functionality to normal
    ///
    /// # Errors
    ///
    /// This function will return an error if the device can not be written to.
    ///
    /// # Panics
    ///
    /// This function will panic if the LED index is out of range. Check
    /// against [`max_status_led_index`](Keyboard::max_status_led_index).
    ///
    /// [`take_led_control(false)`]: Self::take_led_control
    #[allow(
        clippy::panic_in_result_fn,
        reason = "validation on user-specified parameter. also consistency with other functions so they all return the same type"
    )]
    pub fn set_status_led(&self, index: u8, enable: bool) -> Result<(), HidError> {
        assert!(index <= self.max_status_led_index());
        self.write(Command::SetStatusLed { enable, index })
    }

    /// Take control of the status LEDs for yourself. This prevents the keyboard
    /// from setting the LEDs itself, which usually happens when switching
    /// layers.
    ///
    /// This is automatically done for you by the keyboard upon calling
    /// [`set_status_led`](Keyboard::set_status_led), meaning you generally only
    /// need to call this with `false`
    ///
    /// # Errors
    ///
    /// This function will return an error if the device can not be written to.
    pub fn take_led_control(&self, b: bool) -> Result<(), HidError> {
        self.write(Command::TakeLedControl { take_control: b })
    }

    /// Increase the brightness of the LEDs behind the keys on the keyboard.
    ///
    /// This is by a keyboard-defined amount - you may need to call this
    /// multiple times to achieve the result you desire.
    ///
    /// # Errors
    ///
    /// This function will return an error if the device can not be written to.
    ///
    /// See also: [`decrease_brightness`](Keyboard::decrease_brightness)
    pub fn increase_brightness(&self) -> Result<(), HidError> {
        self.write(Command::UpdateBrightness { increase: true })
    }

    /// Decrease the brightness of the LEDs behind the keys on the keyboard.
    ///
    /// This is by a keyboard-defined amount - you may need to call this
    /// multiple times to achieve the result you desire.
    ///
    /// # Errors
    ///
    /// This function will return an error if the device can not be written to.
    ///
    /// See also: [`increase_brightness`](Keyboard::increase_brightness)
    pub fn decrease_brightness(&self) -> Result<(), HidError> {
        self.write(Command::UpdateBrightness { increase: false })
    }

    #[must_use]
    pub const fn max_status_led_index(&self) -> u8 {
        #[allow(clippy::match_same_arms, reason = "different keyboards")]
        match self.pid {
            0x1307 => 2,          // Ergodox EZ
            0x4974 | 0x2010 => 2, // Ergodox EZ Original
            0x4975 | 0x2020 => 2, // Ergodox EZ Shine
            0x4976 | 0x2030 => 2, // Ergodox EZ Glow
            0xc6ce | 0xc6cf => 1, // Planck EZ Standard/Glow
            0x1969 => 5,          // Moonlander MK1
            0x1977 | 0x1978 => 3, // Voyager
            _ => 0,
        }
    }

    /// Read a response from the connected device. If the timeout is exceeded
    /// without a response being received, `Ok(None)` is returned.
    ///
    /// If the timeout is `None`, the read will block until a value is received
    /// (or an error occurs). If the timeout is `Some(Duration::ZERO)`, this
    /// will be a non-blocking read.
    ///
    /// Note that this always ignores [`self.device.set_blocking_mode`](HidDevice::set_blocking_mode).
    ///
    /// # Errors
    ///
    /// This function will return an error if the device cannot be read.
    ///
    /// # Panics
    ///
    /// This function requires the timeout to be accurate to at most a
    /// millisecond. Specifying nanoseconds or microseconds will cause a panic.
    /// In addition, the millisecond must be convertable to an [`i32`]
    #[allow(
        clippy::panic_in_result_fn,
        reason = "validation on user-provided argument"
    )]
    pub fn read(
        &mut self,
        timeout: Option<Duration>,
    ) -> Result<Option<KeyboardResponse>, HidError> {
        let timeout: i32 = timeout.map_or(-1, |timeout| {
            let millis = timeout.as_millis();
            assert_eq!(
                millis.wrapping_mul(1_000_000),
                timeout.as_nanos(),
                "Timeout may only be accurate to a millisecond."
            );
            #[allow(clippy::expect_used, reason = "validation on user-provided argument")]
            millis
                .try_into()
                .expect("Milliseconds cannot be converted to i32")
        });

        let mut bytes = [0; 0x21];
        let read = self.device.read_timeout(&mut bytes, timeout)?;
        if read == 0 {
            return Ok(None);
        }

        let parsed = KeyboardResponse::parse_incoming_packet(bytes);
        match &parsed {
            KeyboardResponse::ProtocolVersion(version) => self.protocol = Some(*version),
            KeyboardResponse::FirmwareVersion(firmware) => {
                self.firmware = Some((&**firmware).into());
            }
            KeyboardResponse::PairingInit(_) => self.write(Command::HandleIncomingPacket)?,
            KeyboardResponse::PairingFail(_) => {
                self.paired = false;
            }
            KeyboardResponse::PairingSuccess(_) => {
                self.paired = true;
            }
            _ => {}
        }

        Ok(Some(parsed))
    }

    /// Read a response from the connected device. If the timeout is exceeded
    /// without a response being received, `Ok(None)` is returned.
    ///
    /// # Errors
    ///
    /// This function will return an error if the device cannot be read.
    ///
    /// # Panics
    ///
    /// This function requires the timeout to be accurate to at most a
    /// millisecond. Specifying nanoseconds or microseconds will cause a panic.
    /// In addition, the millisecond must be convertable to an [`i32`]
    pub fn read_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<KeyboardResponse>, HidError> {
        self.read(Some(timeout))
    }

    /// Attempt to read a response from the connected device. This operation
    /// does not block the current thread.
    ///
    /// This function will return `Ok(None)` if there is no value to be read.
    ///
    /// # Errors
    ///
    /// This function will return an error if the device cannot be read.
    pub fn try_read(&mut self) -> Result<Option<KeyboardResponse>, HidError> {
        self.read(Some(Duration::ZERO))
    }

    /// Read a response from the connected device, blocking the thread until
    /// either a value is received or an error occurs.
    ///
    /// # Errors
    ///
    /// This function will return an error if the device cannot be read.
    #[allow(clippy::missing_panics_doc)]
    #[allow(
        clippy::panic_in_result_fn,
        reason = "blocked reads should never return an Option"
    )]
    pub fn read_blocking(&mut self) -> Result<KeyboardResponse, HidError> {
        self.read(None).map(|x| {
            #[allow(
                clippy::expect_used,
                reason = "blocked reads should never return an Option"
            )]
            x.expect("there to be a value on a blocked read")
        })
    }

    /// Issues a command to the connected device.
    ///
    /// # Errors
    ///
    /// This function will return an error if the device can not be written to.
    pub fn write(&self, command: Command) -> Result<(), HidError> {
        // Note: it must be exactly 0x21 (33) bytes written, else the keyboard
        // ignores the request.
        let mut bytes = [0_u8; 0x21];
        for (i, byte) in command.bytes().iter().enumerate() {
            if let Some(b) = bytes.get_mut(i) {
                *b = *byte;
            }
        }

        let written = self.device.write(&bytes)?;
        debug_assert_eq!(written, 0x21);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardType {
    ErgodoxEz,
    PlanckEz,
    Moonlander,
    Voyager,
}

impl KeyboardType {
    /// Determine the type of ZSA keyboard based on it's vendor and product ID.
    #[must_use]
    pub const fn identify_keyboard(vid: u16, pid: u16) -> Option<Self> {
        /// ZSA vendor ID.
        pub const ZSA_VID: u16 = 0x3297;
        if vid != ZSA_VID {
            return None;
        }
        match pid {
            0x1307 | 0x4974 | 0x4975 | 0x4976 | 0x2010 | 0x2020 | 0x2030 => Some(Self::ErgodoxEz),
            0x6060 | 0xC6CE | 0xC6CF => Some(Self::PlanckEz),
            0x1969 | 0x1972 => Some(Self::Moonlander),
            0x1977 | 0x1978 => Some(Self::Voyager),
            _ => None,
        }
    }
}

/// A command to issue to the keyboard. You likely want to use the methods
/// on [`Keyboard`] instead of instantiating these directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Command {
    RequestFirmwareVersion,
    RequestPairing,
    HandleIncomingPacket,
    SetLayer { layer: u8, activate_layer: bool },
    TakeRgbControl { take_control: bool },
    SetRgbLed { key_index: u8, r: u8, g: u8, b: u8 },
    SetStatusLed { index: u8, enable: bool },
    UpdateBrightness { increase: bool },
    SetRgbAll { r: u8, g: u8, b: u8 },
    TakeLedControl { take_control: bool },
    RequestFirmwareProtocolVersion,
}
impl Command {
    /// The minimum bytes to send for this command.
    ///
    /// Note that all commands are exactly 0x21 bytes long but anything after
    /// the minimum this function returns are ignored. [`Keyboard::write`] sets
    /// these extra bytes to `0` but it could just as well be uninitialized
    /// memory.
    #[must_use]
    pub fn bytes(self) -> Box<[u8]> {
        match self {
            Self::RequestFirmwareVersion => Box::new([0, 0]),
            Self::RequestPairing => Box::new([0, 1]),
            Self::HandleIncomingPacket => Box::new([0, 2]),
            Self::SetLayer {
                layer,
                activate_layer,
            } => Box::new([0, 4, activate_layer.into(), layer]),
            Self::TakeRgbControl { take_control } => Box::new([0, 5, take_control.into()]),
            Self::SetRgbLed { key_index, r, g, b } => Box::new([0, 6, key_index, r, g, b]),
            Self::SetStatusLed { index, enable } => Box::new([0, 7, index, enable.into()]),
            Self::UpdateBrightness { increase } => Box::new([0, 8, increase.into()]),
            Self::SetRgbAll { r, g, b } => Box::new([0, 9, r, g, b]),
            Self::TakeLedControl { take_control } => Box::new([0, 10, take_control.into()]),
            Self::RequestFirmwareProtocolVersion => Box::new([0, 0xfe]),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyEventState {
    Pressed,
    Released,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum KeyboardResponse {
    ProtocolVersion(u8),
    FirmwareVersion(Box<str>),
    PairingInit(Box<[u8]>),
    PairingKeyInput(Box<[u8]>),
    PairingFail(Box<[u8]>),
    PairingSuccess(Box<[u8]>),
    LayerChanged(u8),
    /// Note that sometimes the row and column are a bit nonsensical.
    /// This is simply what the keyboard states.
    KeyEvent {
        row: u8,
        column: u8,
        state: KeyEventState,
    },
    ToggleSmartLayer(Box<[u8]>),
    TriggerSmartLayer(Box<[u8]>),
    Unknown(u8, Box<[u8]>),
}
impl KeyboardResponse {
    /// Identify the incoming packet.
    #[must_use]
    pub fn parse_incoming_packet(data: [u8; 0x21]) -> Self {
        let ty = data[0];

        if ty == 0xfe {
            return Self::ProtocolVersion(data[1]);
        }

        let end = data.iter().position(|x| *x == 0xfe).unwrap_or(data.len());
        let Some(data) = data.get(1..end) else {
            return Self::Unknown(ty, data.into());
        };
        match ty {
            0 => Self::FirmwareVersion(str::from_utf8(data).unwrap_or("unknown error").into()),
            1 => Self::PairingInit(data.into()),
            2 => Self::PairingKeyInput(data.into()),
            3 => Self::PairingFail(data.into()),
            4 => Self::PairingSuccess(data.into()),
            5 => data.first().map_or_else(
                || Self::Unknown(ty, data.into()),
                |layer| Self::LayerChanged(*layer),
            ),
            #[allow(clippy::get_first, reason = "it looks better this way")]
            6 | 7 => match (data.get(0), data.get(1)) {
                (Some(&column), Some(&row)) => Self::KeyEvent {
                    row,
                    column,
                    state: if ty == 6 {
                        KeyEventState::Pressed
                    } else {
                        KeyEventState::Released
                    },
                },
                _ => Self::Unknown(ty, data.into()),
            },
            // 8 is not found in the reverse-engineed system but is sent
            // after taking back RGB controls
            9 => Self::ToggleSmartLayer(data.into()),
            10 => Self::TriggerSmartLayer(data.into()),
            _ => Self::Unknown(ty, data.into()),
        }
    }
}

/// An issue occured reading
#[derive(thiserror::Error, Debug)]
pub enum ReadError {
    /// The timeout was exceeded
    #[error("the timeout for reading was exceeded")]
    TimeoutExceeded,
    /// An issue with the device occured.
    #[error(transparent)]
    Hid(#[from] HidError),
}

/// An issue occured pairing the device.
#[derive(thiserror::Error, Debug)]
pub enum PairingError {
    /// The timeout for pairing was exceeded
    #[error("the timeout for pairing was exceeded")]
    TimeoutExceeded,
    /// The keyboard denied the pairing request
    #[error("the keyboard denied the pairing request: {0:?}")]
    KeyboardDenied(Box<[u8]>),
    /// An issue with the device occured.
    #[error(transparent)]
    Hid(#[from] HidError),
}
impl From<ReadError> for PairingError {
    fn from(value: ReadError) -> Self {
        match value {
            ReadError::TimeoutExceeded => Self::TimeoutExceeded,
            ReadError::Hid(hid_error) => Self::Hid(hid_error),
        }
    }
}
