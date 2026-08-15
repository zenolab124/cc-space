use base64::{engine::general_purpose::STANDARD, Engine as _};
#[cfg(target_os = "macos")]
use std::ffi::{c_char, c_void, CStr};
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::sync::mpsc::{sync_channel, SyncSender};
#[cfg(target_os = "macos")]
use std::time::Duration;

const MAX_CAPTURE_BYTES: usize = 100 * 1024 * 1024;
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

#[cfg(target_os = "macos")]
type SnapshotResult = Result<Vec<u8>, String>;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn monet_take_webview_snapshot(
        webview: *mut c_void,
        context: *mut c_void,
        callback: unsafe extern "C" fn(
            context: *mut c_void,
            bytes: *const u8,
            length: usize,
            error: *const c_char,
        ),
    );
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn receive_snapshot(
    context: *mut c_void,
    bytes: *const u8,
    length: usize,
    error: *const c_char,
) {
    let sender = unsafe { Box::from_raw(context.cast::<SyncSender<SnapshotResult>>()) };
    let result = if !error.is_null() {
        Err(unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned())
    } else if bytes.is_null() || length == 0 {
        Err("WebKit returned an empty snapshot".to_string())
    } else if length > MAX_CAPTURE_BYTES {
        Err("Capture exceeds the 100 MB limit".to_string())
    } else {
        Ok(unsafe { std::slice::from_raw_parts(bytes, length) }.to_vec())
    };
    let _ = sender.try_send(result);
}

fn decode_png_payload(encoded: &str) -> Result<Vec<u8>, String> {
    if encoded.len() > MAX_CAPTURE_BYTES.saturating_mul(4) / 3 + 4 {
        return Err("Capture exceeds the 100 MB limit".to_string());
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| "Capture payload is not valid base64".to_string())?;
    if bytes.len() > MAX_CAPTURE_BYTES {
        return Err("Capture exceeds the 100 MB limit".to_string());
    }
    if !bytes.starts_with(PNG_SIGNATURE) {
        return Err("Capture payload is not a PNG image".to_string());
    }
    Ok(bytes)
}

#[tauri::command]
pub async fn save_workbench_capture(path: PathBuf, png_base64: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = decode_png_payload(&png_base64)?;
        std::fs::write(path, bytes).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn native_workbench_capture_supported() -> bool {
    cfg!(target_os = "macos")
}

#[tauri::command]
pub async fn capture_native_workbench_tile(window: tauri::WebviewWindow) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let (sender, receiver) = sync_channel::<SnapshotResult>(1);
        window
            .with_webview(move |webview| {
                let context = Box::into_raw(Box::new(sender)).cast::<c_void>();
                unsafe { monet_take_webview_snapshot(webview.inner(), context, receive_snapshot) };
            })
            .map_err(|error| error.to_string())?;
        let bytes = tauri::async_runtime::spawn_blocking(move || {
            receiver
                .recv_timeout(Duration::from_secs(15))
                .map_err(|_| "WebKit snapshot timed out".to_string())?
        })
        .await
        .map_err(|error| error.to_string())??;
        if !bytes.starts_with(PNG_SIGNATURE) {
            return Err("WebKit snapshot is not a PNG image".to_string());
        }
        Ok(STANDARD.encode(bytes))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Err("Native workbench capture is only available on macOS".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_png_payload() {
        let encoded = STANDARD.encode(PNG_SIGNATURE);
        assert_eq!(decode_png_payload(&encoded).unwrap(), PNG_SIGNATURE);
    }

    #[test]
    fn rejects_non_png_payload() {
        let encoded = STANDARD.encode(b"plain text");
        assert!(decode_png_payload(&encoded).is_err());
    }
}
