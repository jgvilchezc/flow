//! Clears a system-applied input mute on a CoreAudio device.
//!
//! Something on macOS occasionally flips `kAudioDevicePropertyMute` (input
//! scope) to 1 on the built-in microphone. cpal then delivers all-zero
//! samples and Whisper hallucinates on the silence. Flow is push-to-talk, so
//! the microphone must be live for as long as the hotkey is held; the
//! recorder calls [`ensure_unmuted`] right before it opens the input stream.
//!
//! cpal 0.15 does not expose the underlying `AudioDeviceID`, so the device is
//! re-resolved here by name through the CoreAudio HAL. The name property read
//! here (`kAudioObjectPropertyName`, fourcc `lnam`) is the same one cpal
//! reports from `Device::name()`, so the lookup matches what the recorder logs.

#[cfg(target_os = "macos")]
mod imp {
    use coreaudio_sys::{
        kAudioDevicePropertyMute, kAudioDevicePropertyStreams, kAudioHardwareNoError,
        kAudioHardwarePropertyDevices, kAudioObjectPropertyElementMain, kAudioObjectPropertyName,
        kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeInput, kAudioObjectSystemObject,
        kCFStringEncodingUTF8, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize,
        AudioObjectHasProperty, AudioObjectID, AudioObjectPropertyAddress,
        AudioObjectPropertyScope, AudioObjectPropertySelector, AudioObjectSetPropertyData,
        CFRelease, CFStringGetCString, CFStringRef, OSStatus,
    };
    use std::ffi::CStr;
    use std::os::raw::{c_char, c_void};
    use std::ptr;

    pub use coreaudio_sys::AudioDeviceID;

    /// Largest UTF-8 device name this module will read back from CoreAudio.
    const NAME_BUFFER_LEN: usize = 256;

    fn address(
        selector: AudioObjectPropertySelector,
        scope: AudioObjectPropertyScope,
    ) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress {
            mSelector: selector,
            mScope: scope,
            mElement: kAudioObjectPropertyElementMain,
        }
    }

    fn has_property(object: AudioObjectID, address: &AudioObjectPropertyAddress) -> bool {
        // SAFETY: `address` is a valid, initialised struct that outlives the
        // call; CoreAudio only reads through the pointer.
        unsafe { AudioObjectHasProperty(object, address) != 0 }
    }

    /// Reads a `UInt32`-typed property. `Err` carries the OSStatus.
    fn get_u32(
        object: AudioObjectID,
        address: &AudioObjectPropertyAddress,
    ) -> Result<u32, OSStatus> {
        let mut value: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        // SAFETY: `size` matches the byte length of `value`, so CoreAudio
        // writes at most 4 bytes into a live, aligned `u32`.
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                address,
                0,
                ptr::null(),
                &mut size,
                &mut value as *mut u32 as *mut c_void,
            )
        };
        if status == kAudioHardwareNoError as OSStatus {
            Ok(value)
        } else {
            Err(status)
        }
    }

    /// Writes a `UInt32`-typed property. `Err` carries the OSStatus.
    fn set_u32(
        object: AudioObjectID,
        address: &AudioObjectPropertyAddress,
        value: u32,
    ) -> Result<(), OSStatus> {
        let size = std::mem::size_of::<u32>() as u32;
        // SAFETY: `size` matches the byte length of `value`; CoreAudio only
        // reads through the data pointer for the duration of the call.
        let status = unsafe {
            AudioObjectSetPropertyData(
                object,
                address,
                0,
                ptr::null(),
                size,
                &value as *const u32 as *const c_void,
            )
        };
        if status == kAudioHardwareNoError as OSStatus {
            Ok(())
        } else {
            Err(status)
        }
    }

    /// Reads an array-of-`AudioObjectID` property. Empty on any error.
    fn get_object_ids(
        object: AudioObjectID,
        address: &AudioObjectPropertyAddress,
    ) -> Vec<AudioObjectID> {
        let mut size: u32 = 0;
        // SAFETY: `size` is a live `u32` CoreAudio writes the byte count into.
        let status =
            unsafe { AudioObjectGetPropertyDataSize(object, address, 0, ptr::null(), &mut size) };
        if status != kAudioHardwareNoError as OSStatus || size == 0 {
            return Vec::new();
        }
        let count = size as usize / std::mem::size_of::<AudioObjectID>();
        let mut ids = vec![0 as AudioObjectID; count];
        // SAFETY: `ids` holds exactly `size` bytes (count * 4), so CoreAudio
        // cannot write past the end of the buffer; `size` is updated in place
        // with the number of bytes actually written.
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                address,
                0,
                ptr::null(),
                &mut size,
                ids.as_mut_ptr() as *mut c_void,
            )
        };
        if status != kAudioHardwareNoError as OSStatus {
            return Vec::new();
        }
        ids.truncate(size as usize / std::mem::size_of::<AudioObjectID>());
        ids
    }

    /// Reads a `CFStringRef`-typed property as a Rust `String`. `None` when
    /// the property is unreadable or not valid UTF-8 within the buffer.
    fn get_string(object: AudioObjectID, address: &AudioObjectPropertyAddress) -> Option<String> {
        let mut string: CFStringRef = ptr::null();
        let mut size = std::mem::size_of::<CFStringRef>() as u32;
        // SAFETY: `size` matches the byte length of `string`, so CoreAudio
        // writes one pointer into a live, aligned `CFStringRef` slot.
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                address,
                0,
                ptr::null(),
                &mut size,
                &mut string as *mut CFStringRef as *mut c_void,
            )
        };
        if status != kAudioHardwareNoError as OSStatus || string.is_null() {
            return None;
        }
        let mut buffer = [0 as c_char; NAME_BUFFER_LEN];
        // SAFETY: `string` is a +1 retained CFString handed to us by the HAL
        // (the caller owns it, so it is released exactly once below);
        // `buffer.len()` is passed as the capacity, so CFStringGetCString
        // NUL-terminates within bounds or reports failure.
        let converted = unsafe {
            let ok = CFStringGetCString(
                string,
                buffer.as_mut_ptr(),
                buffer.len() as _,
                kCFStringEncodingUTF8,
            );
            CFRelease(string as *const c_void);
            ok != 0
        };
        if !converted {
            return None;
        }
        // SAFETY: CFStringGetCString succeeded, so `buffer` holds a
        // NUL-terminated C string within its bounds.
        let name = unsafe { CStr::from_ptr(buffer.as_ptr()) };
        name.to_str().ok().map(str::to_owned)
    }

    /// True when the device exposes at least one input stream.
    fn has_input_streams(device: AudioDeviceID) -> bool {
        let streams = address(kAudioDevicePropertyStreams, kAudioObjectPropertyScopeInput);
        !get_object_ids(device, &streams).is_empty()
    }

    /// Finds the CoreAudio device id of the input device called `name`.
    /// Returns the first match when several devices share a name.
    pub fn find_input_device_id(name: &str) -> Option<AudioDeviceID> {
        let devices = address(
            kAudioHardwarePropertyDevices,
            kAudioObjectPropertyScopeGlobal,
        );
        let name_address = address(kAudioObjectPropertyName, kAudioObjectPropertyScopeGlobal);
        get_object_ids(kAudioObjectSystemObject as AudioObjectID, &devices)
            .into_iter()
            .filter(|&device| has_input_streams(device))
            .find(|&device| get_string(device, &name_address).as_deref() == Some(name))
    }

    /// Clears the input-scope mute on the device called `name`.
    ///
    /// Returns `Ok(true)` when the device was muted and is now unmuted,
    /// `Ok(false)` when there was nothing to do (device not found, no mute
    /// control, or already unmuted), and `Err` when CoreAudio refused the
    /// write or the mute did not clear on read-back.
    pub fn ensure_unmuted(name: &str) -> Result<bool, String> {
        let Some(device) = find_input_device_id(name) else {
            return Ok(false);
        };
        let mute = address(kAudioDevicePropertyMute, kAudioObjectPropertyScopeInput);
        if !has_property(device, &mute) {
            return Ok(false);
        }
        match get_u32(device, &mute) {
            Ok(1) => {}
            Ok(_) | Err(_) => return Ok(false),
        }
        set_u32(device, &mute, 0).map_err(|status| {
            format!("AudioObjectSetPropertyData(mute=0) failed with OSStatus {status}")
        })?;
        match get_u32(device, &mute) {
            Ok(0) => Ok(true),
            Ok(value) => Err(format!("mute still reads {value} after clearing it")),
            Err(status) => Err(format!(
                "mute cleared but read-back failed with OSStatus {status}"
            )),
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    /// Placeholder for the CoreAudio device id on platforms without CoreAudio.
    pub type AudioDeviceID = u32;

    /// Mirrors the macOS API surface; only exercised by the macOS tests.
    #[allow(dead_code)]
    pub fn find_input_device_id(_name: &str) -> Option<AudioDeviceID> {
        None
    }

    pub fn ensure_unmuted(_name: &str) -> Result<bool, String> {
        Ok(false)
    }
}

pub use imp::ensure_unmuted;

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::imp::{ensure_unmuted, find_input_device_id};

    #[test]
    fn find_input_device_id_returns_none_for_unknown_name() {
        assert_eq!(
            find_input_device_id("Flow Nonexistent Microphone 8f3a"),
            None
        );
    }

    #[test]
    fn ensure_unmuted_on_missing_device_returns_ok_false() {
        assert_eq!(
            ensure_unmuted("Flow Nonexistent Microphone 8f3a"),
            Ok(false)
        );
    }

    /// Manual end-to-end check against the real hardware. Mute the microphone
    /// first (`~/.flow-diag/micwatch --mute`), then run:
    /// `cargo test --lib manual_unmute_roundtrip -- --ignored --nocapture`
    /// and confirm `micwatch --status` reports `mute=0` afterwards.
    #[test]
    #[ignore]
    fn manual_unmute_roundtrip() {
        let result = ensure_unmuted("MacBook Pro Microphone");
        println!("ensure_unmuted(\"MacBook Pro Microphone\") = {result:?}");
        assert!(result.is_ok(), "unmute failed: {result:?}");
    }
}
