use std::{sync::mpsc, time::Duration};

use hidapi::{HidApi, HidDevice};

fn main() {
    let api = HidApi::new().unwrap();
    let (kbloop, kb) = find_keyboard(&api);
    println!("Found {:?} connected", kb);
    std::thread::spawn(|| {
        kbloop.start_loop();
    });

    kb.request_pairing();
    kb.request_firmware_version();
    kb.request_firmware_protocol_version();
    std::thread::sleep(Duration::from_secs(1));
    kb.set_layer(5, true);
    // kb.set_layer(1, true);
    std::thread::sleep(Duration::from_secs(100));
}

pub fn find_keyboard(api: &HidApi) -> (KeyboardLoop, Keyboard) {
    for dev_info in api.device_list() {
        let vid = dev_info.vendor_id();
        let pid = dev_info.product_id();
        println!("{vid} / {pid} / {:?}", dev_info.path());
        if dev_info.path() != c"/dev/hidraw8" {
            continue;
        }

        if let Some(keyboard) = KeyboardType::identify_keyboard(vid, pid) {
            let serial = dev_info.serial_number().unwrap_or_default().to_string();
            let device = api.open_path(dev_info.path()).expect("couldnt take");
            let (s, r) = mpsc::channel();
            let kbloop = KeyboardLoop { device, queue: r };
            let kb = Keyboard {
                sender: s,
                keyboard,
                serial,
                pid,
            };
            return (kbloop, kb);
        }
    }
    panic!("no keyboard found");
}

pub struct KeyboardLoop {
    pub device: HidDevice,
    pub queue: mpsc::Receiver<Command>,
}
impl KeyboardLoop {
    pub fn start_loop(self) {
        self.device.set_blocking_mode(false).unwrap();

        loop {
            for cmd in self.queue.try_iter() {
                self.write(cmd);
            }

            let mut bytes = [0; 0x21];
            let read = self.device.read(&mut bytes).unwrap();
            if read == 0 {
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            // println!("got packet: {bytes:02x?}");
            let parsed = KeyboardResponse::parse_incoming_packet(&bytes);
            println!("{parsed:?}");
            if let KeyboardResponse::PairingInit(b) = parsed {
                self.write(Command::HandleIncomingPacket);
            }
        }
    }

    pub fn write(&self, command: Command) {
        let mut bytes = [0_u8; 0x21];
        for (i, byte) in command.bytes().iter().enumerate() {
            bytes[i] = *byte;
        }
        println!("Writing {command:?} ({:02x?})", bytes);
        let written = self.device.write(&bytes).unwrap();
        assert_eq!(written, 0x21);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyEventState {
    Pressed,
    Released,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyboardResponse<'a> {
    ProtocolVersion(u8),
    FirmwareVersion(&'a [u8]),
    PairingInit(&'a [u8]),
    PairingKeyInput(&'a [u8]),
    PairingFail(&'a [u8]),
    PairingSuccess(&'a [u8]),
    LayerChanged(u8),
    /// Note that sometimes the row and column are a bit nonsensical.
    /// This is simply what the keyboard states.
    KeyEvent {
        row: u8,
        column: u8,
        state: KeyEventState,
    },
    ToggleSmartLayer(&'a [u8]),
    TriggerSmartLayer(&'a [u8]),
}
impl<'a> KeyboardResponse<'a> {
    fn parse_incoming_packet(data: &'a [u8; 0x21]) -> KeyboardResponse<'a> {
        let ty = data[0];

        if ty == 0xfe {
            return KeyboardResponse::ProtocolVersion(data[1]);
        }

        let end = data.iter().position(|x| *x == 0xfe).unwrap_or(data.len());
        let data = &data[1..end];
        match ty {
            0 => KeyboardResponse::FirmwareVersion(data),
            1 => KeyboardResponse::PairingInit(data),
            2 => KeyboardResponse::PairingKeyInput(data),
            3 => KeyboardResponse::PairingFail(data),
            4 => KeyboardResponse::PairingSuccess(data),
            5 => KeyboardResponse::LayerChanged(data[0]),
            6 | 7 => KeyboardResponse::KeyEvent {
                row: data[1],
                column: data[0],
                state: if ty == 6 {
                    KeyEventState::Pressed
                } else {
                    KeyEventState::Released
                },
            },
            // i think 8 isnt used
            9 => KeyboardResponse::ToggleSmartLayer(data),
            10 => KeyboardResponse::TriggerSmartLayer(data),
            _ => panic!("unknown type"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Command {
    RequestFirmwareVersion,
    RequestPairing,
    HandleIncomingPacket,
    SetLayer { layer: u8, activate_layer: bool },
    TakeRgbControl { b: bool },
    SetRgbLed { a: u8, b: u8, c: u8, d: u8 },
    SetStatusLed { index: u8, b: bool },
    UpdateBrightness { increase: bool },
    SetRgbAll { a: u8, b: u8, c: u8 },
    TakeLedControl { b: bool },
    RequestFirmwareProtocolVersion,
}
impl Command {
    pub fn bytes(&self) -> Box<[u8]> {
        match self {
            Command::RequestFirmwareVersion => Box::new([0, 0]),
            Command::RequestPairing => Box::new([0, 1]),
            Command::HandleIncomingPacket => Box::new([0, 2]),
            Command::SetLayer {
                layer,
                activate_layer,
            } => Box::new([0, 4, *activate_layer as u8, *layer]),
            Command::TakeRgbControl { b } => Box::new([0, 5, *b as u8]),
            Command::SetRgbLed { a, b, c, d } => Box::new([0, 6, *a, *b, *c, *d]),
            Command::SetStatusLed { index, b } => Box::new([0, 7, *index, *b as u8]),
            Command::UpdateBrightness { increase } => Box::new([0, 8, *increase as u8]),
            Command::SetRgbAll { a, b, c } => Box::new([0, 9, *a, *b, *c]),
            Command::TakeLedControl { b } => Box::new([0, 10, *b as u8]),
            Command::RequestFirmwareProtocolVersion => Box::new([0, 0xfe]),
        }
    }
}

#[derive(Debug)]
pub struct Keyboard {
    pub keyboard: KeyboardType,
    pub serial: String,
    pub pid: u16,
    pub sender: mpsc::Sender<Command>,
}
impl Keyboard {
    pub fn request_pairing(&self) {
        self.send(Command::RequestPairing);
    }

    pub fn request_firmware_version(&self) {
        self.send(Command::RequestFirmwareVersion);
    }

    pub fn request_firmware_protocol_version(&self) {
        self.send(Command::RequestFirmwareProtocolVersion);
    }

    pub fn set_layer(&self, layer: u8, activate_layer: bool) {
        self.send(Command::SetLayer {
            layer,
            activate_layer,
        });
    }

    pub fn take_rgb_control(&self, b: bool) {
        self.send(Command::TakeRgbControl { b });
    }

    pub fn set_rgb_led(&self, a: u8, b: u8, c: u8, d: u8) {
        self.send(Command::SetRgbLed { a, b, c, d });
    }

    pub fn set_rgb_all(&self, a: u8, b: u8, c: u8) {
        self.send(Command::SetRgbAll { a, b, c });
    }

    pub fn set_status_led(&self, index: u8, b: bool) {
        assert!(index <= self.max_status_led_index());
        self.send(Command::SetStatusLed { b, index });
    }

    pub fn take_led_control(&self, b: bool) {
        self.send(Command::TakeLedControl { b });
    }

    /// `true` to increase brightness, `false` to decrease brightness
    pub fn update_brightness(&self, increase: bool) {
        self.send(Command::UpdateBrightness { increase });
    }

    pub fn max_status_led_index(&self) -> u8 {
        match self.pid {
            0x1307 => 2,          // Ergodox EZ
            0x4974 | 0x2010 => 2, // Ergodox EZ Original
            0x4975 | 0x2020 => 2, // Ergodox EZ Shine
            0x4976 | 0x2030 => 2, // Ergodox EZ Glow
            0xc6ce | 0xc6cf => 1, // Planck EZ Standard/Glow
            0x1969 => 5,          // Moonlander MK1
            0x1977 | 0x1978 => 2, // Voyager
            _ => 0,
        }
    }

    fn send(&self, cmd: Command) {
        self.sender.send(cmd).unwrap();
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
    pub fn identify_keyboard(vid: u16, pid: u16) -> Option<KeyboardType> {
        /// ZSA vendor ID.
        pub const ZSA_VID: u16 = 0x3297;
        if vid != ZSA_VID {
            return None;
        }
        match pid {
            0x1307 | 0x4974 | 0x4975 | 0x4976 | 0x2010 | 0x2020 | 0x2030 => {
                Some(KeyboardType::ErgodoxEz)
            }
            0x6060 | 0xC6CE | 0xC6CF => Some(KeyboardType::PlanckEz),
            0x1969 | 0x1972 => Some(KeyboardType::Moonlander),
            0x1977 | 0x1978 => Some(KeyboardType::Voyager),
            _ => None,
        }
    }
}
