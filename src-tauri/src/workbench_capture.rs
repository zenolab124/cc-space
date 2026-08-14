use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::path::PathBuf;

const MAX_CAPTURE_BYTES: usize = 100 * 1024 * 1024;
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

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
