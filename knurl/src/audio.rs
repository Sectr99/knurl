//! A program's own volume in the Windows volume mixer (its audio sessions), on every active output device.
//! This is software mixing, so it works even when the output device ignores Windows' master volume.
use crate::apps;
use windows::Win32::{
    Media::Audio::*,
    System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED},
};
use windows::core::Interface;

/// Once at startup, on the UI thread (WebView2 needs the same single-threaded apartment).
pub fn init() { unsafe { let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok(); } }

/// Calls `f(program, session)` for every program's mixer session (system sounds skipped), on all active outputs.
fn each_session(mut f: impl FnMut(&str, &IAudioSessionControl)) {
    unsafe {
        let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return };
        let Ok(devices) = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return };
        for d in 0..devices.GetCount().unwrap_or(0) {
            let Ok(mgr) = devices.Item(d).and_then(|dev| dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None)) else { continue };
            let Ok(list) = mgr.GetSessionEnumerator() else { continue };
            for i in 0..list.GetCount().unwrap_or(0) {
                let Ok(ctl) = list.GetSession(i) else { continue };
                let pid = ctl.cast::<IAudioSessionControl2>().and_then(|c| c.GetProcessId()).unwrap_or(0);
                if pid == 0 { continue; }
                if let Some(exe) = apps::exe_of_pid(pid) { f(&exe, &ctl); }
            }
        }
    }
}

/// Every mixer session belonging to `app` (e.g. "discord"), across all active output devices.
fn sessions(app: &str) -> Vec<ISimpleAudioVolume> {
    let mut found = vec![];
    each_session(|exe, ctl| if exe == app { if let Ok(v) = ctl.cast::<ISimpleAudioVolume>() { found.push(v); } });
    found
}

/// The program "Media Volume" controls: the one behind Windows' current media session (what the media keys
/// play/pause), else whatever is making sound right now (some players' session ids don't name the program).
pub fn media_app() -> Option<String> {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager as Media;
    let id = (|| -> windows::core::Result<String> {
        Ok(Media::RequestAsync()?.join()?.GetCurrentSession()?.SourceAppUserModelId()?.to_string())
    })().unwrap_or_default();
    // "Spotify.exe" for desktop programs, "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify" for Store apps
    let name = id.rsplit('!').next().unwrap_or("").to_lowercase().trim_end_matches(".exe").to_string();
    if !name.is_empty() && !sessions(&name).is_empty() { return Some(name); }
    let mut playing = None;
    each_session(|exe, ctl| if playing.is_none() && unsafe { ctl.GetState() } == Ok(AudioSessionStateActive) { playing = Some(exe.to_string()); });
    playing
}

/// Nudge `app`'s volume by `delta` (0.05 = 5 points), or toggle its mute. All its sessions are set to the same
/// level so they stay in step. Returns the new volume (0-100) and mute state, or None if it has no audio yet.
pub fn change(app: &str, delta: f32, toggle_mute: bool) -> Option<(u32, bool)> {
    let all = sessions(app);
    let first = all.first()?;
    unsafe {
        let level = (first.GetMasterVolume().ok()? + delta).clamp(0.0, 1.0);
        let muted = first.GetMute().ok()?.as_bool() ^ toggle_mute;
        for s in &all {
            if delta != 0.0 { let _ = s.SetMasterVolume(level, std::ptr::null()); }
            if toggle_mute { let _ = s.SetMute(muted, std::ptr::null()); }
        }
        Some(((level * 100.0).round() as u32, muted))
    }
}

/// Current volume (0-100) and mute state, without changing anything.
#[cfg(test)]
pub fn get(app: &str) -> Option<(u32, bool)> { change(app, 0.0, false) }

#[cfg(test)]
mod tests {
    // Reads (and restores) a real program's mixer volume: cargo test reads_discord_mixer -- --ignored --nocapture
    #[test]
    #[ignore]
    fn reads_discord_mixer() {
        super::init();
        let before = super::get("discord");
        println!("discord mixer: {before:?}");
        let (v, _) = before.expect("Discord has no mixer session; is it running and has it played sound?");
        let after = super::change("discord", -0.05, false).unwrap();
        assert_eq!(after.0, v.saturating_sub(5));
        assert_eq!(super::change("discord", 0.05, false).unwrap().0.min(100), v.min(100));   // put it back
    }
}
