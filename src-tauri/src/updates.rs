use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
#[cfg(target_os = "macos")]
use std::ffi::{CStr, CString};
use std::time::Duration;

const RELEASES: &str = "https://github.com/AndroidSix/mac-space/releases";

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn macspace_github_token_read() -> *mut libc::c_char;
    fn macspace_github_token_write(token: *const libc::c_char) -> i32;
}

#[cfg(target_os = "macos")]
fn token() -> String {
    unsafe {
        let value = macspace_github_token_read();
        if value.is_null() {
            return String::new();
        }
        let result = CStr::from_ptr(value).to_string_lossy().into_owned();
        libc::free(value.cast());
        result
    }
}

#[cfg(windows)]
fn token() -> String {
    crate::windows::read_token()
}

#[tauri::command]
pub async fn has_update_token() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(|| !token().is_empty())
        .await
        .map_err(|_| "update_failed".into())
}

#[tauri::command]
pub async fn save_update_token(value: String) -> Result<(), String> {
    if value.len() > 512 || value.chars().any(char::is_whitespace) {
        return Err("invalid_token".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(windows)]
        {
            return crate::windows::write_token(&value);
        }
        #[cfg(target_os = "macos")]
        {
            let value = CString::new(value).map_err(|_| "invalid_token")?;
            if unsafe { macspace_github_token_write(value.as_ptr()) } != 0 {
                return Err("keychain_failed".into());
            }
            Ok(())
        }
    })
    .await
    .map_err(|_| "keychain_failed".to_string())?
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    current: String,
    latest: String,
    available: bool,
    url: String,
    checked_at: u64,
}

fn compare_release(release: Release) -> Result<UpdateInfo, String> {
    let current = env!("CARGO_PKG_VERSION");
    let latest = release.tag_name.trim_start_matches('v');
    let version = semver::Version::parse(latest).map_err(|_| "update_invalid")?;
    let installed = semver::Version::parse(current).map_err(|_| "update_invalid")?;
    let url = reqwest::Url::parse(&release.html_url).map_err(|_| "update_invalid")?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || !url
            .path()
            .starts_with("/AndroidSix/mac-space/releases/tag/")
    {
        return Err("update_invalid".into());
    }
    Ok(UpdateInfo {
        current: current.into(),
        latest: latest.into(),
        available: version > installed,
        url: release.html_url,
        checked_at: crate::now(),
    })
}

#[tauri::command]
pub async fn check_update() -> Result<UpdateInfo, String> {
    let credential = tauri::async_runtime::spawn_blocking(token)
        .await
        .map_err(|_| "keychain_failed")?;
    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "update_failed")?;
    let mut request = client
        .get("https://api.github.com/repos/AndroidSix/mac-space/releases/latest")
        .header(
            "User-Agent",
            concat!("SystemBalance/", env!("CARGO_PKG_VERSION")),
        )
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if !credential.is_empty() {
        request = request.bearer_auth(credential);
    }
    let response = request.send().await.map_err(|_| "update_network")?;
    match response.status() {
        StatusCode::NOT_FOUND | StatusCode::UNAUTHORIZED => return Err("update_auth".into()),
        StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS => {
            return Err("update_limited".into());
        }
        status if !status.is_success() => return Err("update_failed".into()),
        _ => {}
    }
    compare_release(response.json().await.map_err(|_| "update_invalid")?)
}

#[tauri::command]
pub fn open_releases() -> Result<(), String> {
    crate::open_external(RELEASES)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checks_semantic_versions_and_release_origin() {
        let release = |tag: &str| Release {
            tag_name: tag.into(),
            html_url: format!("{RELEASES}/tag/{tag}"),
        };
        assert!(compare_release(release("v0.10.0")).unwrap().available);
        assert!(!compare_release(release("v0.2.0")).unwrap().available);
        assert!(!compare_release(release("v0.3.0")).unwrap().available);
        assert!(compare_release(release("invalid")).is_err());
        assert!(
            compare_release(Release {
                tag_name: "v1.0.0".into(),
                html_url: "https://example.org".into()
            })
            .is_err()
        );
    }
}
