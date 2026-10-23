//! Machine and device plumbing: autostart, install, battery level, and writing the knob's own key mapping.
use crate::config::APP_NAME;
use std::path::PathBuf;
use windows_sys::{
    core::GUID,
    Win32::{
        Devices::DeviceAndDriverInstallation::*,
        Devices::Properties::{DEVPKEY_Device_FriendlyName, DEVPROPKEY},
        System::Registry::{RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ},
        UI::Input::{GetRawInputData, GetRawInputDeviceInfoW, RegisterRawInputDevices, HRAWINPUT, RAWINPUTDEVICE,
                    RAWINPUTHEADER, RIDEV_INPUTSINK, RIDI_DEVICENAME, RID_INPUT, RIM_TYPEHID},
    },
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const KNOB_VID: u16 = 0x514C;
const KNOB_PID: u16 = 0x8850;
const KNOB_BT_NAME: &str = "ANTICATER";      // Bluetooth friendly name starts with this
// {104EA319-6EE2-4701-BD47-8DDBF425BBE5} 2: battery percentage on Bluetooth LE devices (what Settings shows)
const DEVPKEY_BATTERY: DEVPROPKEY = DEVPROPKEY { fmtid: GUID::from_u128(0x104EA319_6EE2_4701_BD47_8DDBF425BBE5), pid: 2 };
// {83DA6326-97A6-4088-9453-A1923F573B29} 15: DEVPKEY_Device_IsConnected
const DEVPKEY_CONNECTED: DEVPROPKEY = DEVPROPKEY { fmtid: GUID::from_u128(0x83DA6326_97A6_4088_9453_A1923F573B29), pid: 15 };

pub fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }

// --- autostart / install ---

pub fn startup_enabled() -> bool {
    unsafe { RegGetValueW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), wide(APP_NAME).as_ptr(), RRF_RT_REG_SZ,
                          std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) == 0 }
}

/// Points at the installed copy when there is one, so rebuilding or moving the project can't break autostart.
pub fn set_startup(on: bool) {
    unsafe {
        if on {
            let target = if installed_exe().exists() { installed_exe() } else { std::env::current_exe().unwrap() };
            let exe = wide(&format!("\"{}\"", target.display()));
            RegSetKeyValueW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), wide(APP_NAME).as_ptr(), REG_SZ,
                            exe.as_ptr().cast(), (exe.len() * 2) as u32);
        } else {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), wide(APP_NAME).as_ptr());
        }
    }
}

pub fn installed_exe() -> PathBuf {
    PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default()).join("Programs").join(APP_NAME).join("knob-tray.exe")
}

pub fn running_installed() -> bool {
    std::env::current_exe().ok().and_then(|p| p.canonicalize().ok()) == installed_exe().canonicalize().ok()
}

/// Copy this exe to %LOCALAPPDATA%\Programs\RotaryKnob and add a Start-menu shortcut.
pub fn install() -> Result<PathBuf, String> {
    let dest = installed_exe();
    std::fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::copy(std::env::current_exe().map_err(|e| e.to_string())?, &dest).map_err(|e| format!("copy failed: {e}"))?;
    let lnk = PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join(r"Microsoft\Windows\Start Menu\Programs\Rotary Knob.lnk");
    // ponytail: shortcut via PowerShell's WScript.Shell; a native IShellLink needs the full COM bindings
    let script = format!("$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}');$s.TargetPath='{}';$s.Save()",
                         lnk.display().to_string().replace('\'', "''"), dest.display().to_string().replace('\'', "''"));
    use std::os::windows::process::CommandExt;
    let ok = std::process::Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(0x0800_0000).status().map(|s| s.success()).unwrap_or(false);   // CREATE_NO_WINDOW
    if !ok { return Err("installed, but the Start-menu shortcut couldn't be created".into()); }
    if startup_enabled() { set_startup(true); }
    Ok(dest)
}

// --- battery ---

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Battery { pub percent: u8, pub connected: bool }

/// The paired knob's Bluetooth battery level, if Windows knows it.
pub fn battery() -> Option<Battery> {
    unsafe {
        let set = SetupDiGetClassDevsW(std::ptr::null(), wide("BTHLE").as_ptr(), 0, DIGCF_ALLCLASSES | DIGCF_PRESENT);
        if set == -1 { return None; }   // INVALID_HANDLE_VALUE
        let mut found = None;
        let mut i = 0;
        loop {
            let mut dev: SP_DEVINFO_DATA = std::mem::zeroed();
            dev.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
            if SetupDiEnumDeviceInfo(set, i, &mut dev) == 0 { break; }
            i += 1;
            let mut buf = [0u8; 512];
            let prop = |key: &DEVPROPKEY, buf: &mut [u8]| {
                let (mut ty, mut need) = (0u32, 0u32);
                SetupDiGetDevicePropertyW(set, &dev, key, &mut ty, buf.as_mut_ptr(), buf.len() as u32, &mut need, 0) != 0
            };
            if !prop(&DEVPKEY_Device_FriendlyName, &mut buf) { continue; }
            let name: Vec<u16> = buf.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
            if !String::from_utf16_lossy(&name).starts_with(KNOB_BT_NAME) { continue; }
            if prop(&DEVPKEY_BATTERY, &mut buf) {
                let percent = buf[0];
                let connected = prop(&DEVPKEY_CONNECTED, &mut buf) && buf[0] != 0;
                found = Some(Battery { percent, connected });
                break;
            }
        }
        SetupDiDestroyDeviceInfoList(set);
        found
    }
}

// --- the knob's own key mapping (vendor HID protocol, see remap.py) ---

/// Map the knob to F13 (counter-clockwise), F14 (press), F15 (clockwise). USB only.
pub fn setup_knob() -> Result<(), String> {
    let api = hidapi::HidApi::new().map_err(|e| e.to_string())?;
    let info = api.device_list()
        .find(|d| d.vendor_id() == KNOB_VID && d.product_id() == KNOB_PID && d.usage_page() == 0xFF00)
        .ok_or("Knob not found on USB. Plug it in with the cable and try again.")?;
    let dev = info.open_device(&api).map_err(|e| e.to_string())?;
    // 03 FD <key> 01 01 00 01 00 32 <HID code> 00: key 2 = counter-clockwise, 3 = press, 4 = clockwise
    for (key, code) in [(2u8, 0x68u8), (3, 0x69), (4, 0x6A)] {
        let mut p = [0u8; 65];
        p[..11].copy_from_slice(&[0x03, 0xFD, key, 0x01, 0x01, 0x00, 0x01, 0x00, 0x32, code, 0x00]);
        dev.write(&p).map_err(|e| format!("write failed: {e}"))?;
    }
    let mut commit = [0u8; 65];
    commit[..4].copy_from_slice(&[0x03, 0xFD, 0xFE, 0xFF]);
    dev.write(&commit).map_err(|e| format!("write failed: {e}"))?;
    Ok(())
}

/// Ask Windows to send this window the knob's media-key reports (only seen when it still has the factory mapping).
pub fn watch_factory_mapping(hwnd: isize) {
    let rid = RAWINPUTDEVICE { usUsagePage: 0x0C, usUsage: 0x01, dwFlags: RIDEV_INPUTSINK, hwndTarget: hwnd };
    unsafe { RegisterRawInputDevices(&rid, 1, std::mem::size_of::<RAWINPUTDEVICE>() as u32) };
}

/// For WM_INPUT: true when it's the USB knob sending Volume Up/Down/Mute, i.e. it needs setting up.
pub fn is_factory_knob_report(lparam: isize) -> bool {
    unsafe {
        let hdr = std::mem::size_of::<RAWINPUTHEADER>() as u32;
        let mut size = 0u32;
        GetRawInputData(lparam as HRAWINPUT, RID_INPUT, std::ptr::null_mut(), &mut size, hdr);
        let mut buf = vec![0u8; size as usize];
        if GetRawInputData(lparam as HRAWINPUT, RID_INPUT, buf.as_mut_ptr().cast(), &mut size, hdr) != size || size < hdr + 11 { return false; }
        let header = &*(buf.as_ptr() as *const RAWINPUTHEADER);
        if header.dwType != RIM_TYPEHID { return false; }
        let mut n = 0u32;
        GetRawInputDeviceInfoW(header.hDevice, RIDI_DEVICENAME, std::ptr::null_mut(), &mut n);
        let mut name = vec![0u16; n as usize];
        GetRawInputDeviceInfoW(header.hDevice, RIDI_DEVICENAME, name.as_mut_ptr().cast(), &mut n);
        if !String::from_utf16_lossy(&name).to_ascii_uppercase().contains("VID_514C") { return false; }
        let report = &buf[hdr as usize + 8..];                 // after RAWHID's dwSizeHid and dwCount
        report[0] == 5 && matches!(u16::from_le_bytes([report[1], report[2]]), 0xE9 | 0xEA | 0xE2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Hardware/system checks, skipped by default: cargo test -- --ignored --nocapture --test-threads=1

    #[test]
    #[ignore]
    fn reads_knob_battery() {
        let b = battery();
        println!("battery: {b:?}");
        assert!(b.is_some_and(|b| b.percent <= 100));
    }

    #[test]
    #[ignore]
    fn writes_knob_mapping_over_usb() {
        setup_knob().unwrap();
    }

    #[test]
    #[ignore]
    fn installs_into_given_folders() {
        assert!(!startup_enabled(), "would repoint the real autostart entry");
        let tmp = std::env::temp_dir().join("knob-install-test");
        let _ = std::fs::remove_dir_all(&tmp);
        unsafe {
            std::env::set_var("LOCALAPPDATA", tmp.join("local"));
            std::env::set_var("APPDATA", tmp.join("roaming"));
        }
        std::fs::create_dir_all(tmp.join(r"roaming\Microsoft\Windows\Start Menu\Programs")).unwrap();
        let dest = install().unwrap();
        assert!(dest.starts_with(&tmp) && dest.exists());
        assert!(tmp.join(r"roaming\Microsoft\Windows\Start Menu\Programs\Rotary Knob.lnk").exists());
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
