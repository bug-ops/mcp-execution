//! Acceptance test of the helper pipe's access control on Windows (#617): the pipe `bind_pipe`
//! creates (through `HelperServer::bind(Carrier::NamedPipe, ..)`) carries a protected DACL with
//! exactly one allow ACE for OWNER RIGHTS, so another local user cannot open the single instance.
//!
//! The DACL is read back from the live pipe: `GetSecurityInfo` on a client handle opened with
//! `READ_CONTROL` only, then converted to SDDL. The default DACL of a named pipe is not
//! protected and has several ACEs, so it fails every assertion. The rights field is not pinned:
//! `GA` is mapped to specific rights when the descriptor is applied.
//!
//! Windows only. It cannot run on a Unix host; CI's windows-latest leg is the oracle. It does not
//! prove that a second account is denied, only that the descriptor says so.

#![cfg(windows)]
#![expect(
    unsafe_code,
    reason = "reads the DACL of a live pipe through GetSecurityInfo (#617); no safe API"
)]

use std::os::windows::fs::OpenOptionsExt as _;
use std::os::windows::io::AsRawHandle as _;
use std::ptr::null_mut;
use std::time::Duration;

use mcp_execution_cli::runtime::helper::{Carrier, DryRunMode, HelperConfig, HelperServer};
use mcp_execution_core::helper_protocol::HELPER_ENV;
use mcp_execution_introspector::CallTimeout;
use windows_sys::Win32::Foundation::{ERROR_SUCCESS, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
    SE_KERNEL_OBJECT,
};
use windows_sys::Win32::Security::{ACL, DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};

/// `READ_CONTROL`: permission to read the security descriptor, nothing else.
const READ_CONTROL: u32 = 0x0002_0000;

/// The DACL of the object behind `handle` as an SDDL string.
fn dacl_sddl(handle: HANDLE) -> String {
    let mut dacl: *mut ACL = null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: `handle` is a live handle opened with READ_CONTROL; the out-pointers are valid
    // writable locals and the unrequested owner, group and SACL outputs are null.
    let status = unsafe {
        GetSecurityInfo(
            handle,
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &raw mut dacl,
            null_mut(),
            &raw mut descriptor,
        )
    };
    assert_eq!(status, ERROR_SUCCESS, "GetSecurityInfo failed");

    let mut wide: *mut u16 = null_mut();
    // SAFETY: `descriptor` was allocated by the successful call above; `wide` is a valid
    // writable local and the length output is not requested.
    let converted = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &raw mut wide,
            null_mut(),
        )
    };
    assert_ne!(converted, 0, "the SDDL conversion failed");
    // SAFETY: on success `wide` points to a NUL-terminated UTF-16 string we own until LocalFree.
    let text = unsafe {
        let mut len = 0;
        while *wide.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(wide, len))
    };
    // SAFETY: both blocks were allocated by the calls above and are not used afterwards.
    unsafe {
        LocalFree(wide.cast());
        LocalFree(descriptor);
    }
    text
}

#[tokio::test]
async fn the_helper_pipe_dacl_is_protected_with_one_owner_only_allow_ace() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, endpoint) = HelperServer::bind(
        Carrier::NamedPipe,
        HelperConfig {
            servers_dir: dir.path().to_path_buf(),
            call_timeout: CallTimeout::new(Duration::from_secs(30)).unwrap(),
            journal: None,
            dry_run: DryRunMode::Off,
        },
    )
    .expect("bind the helper pipe");
    let descriptor = endpoint
        .env()
        .into_iter()
        .find_map(|(name, value)| (name == HELPER_ENV).then_some(value))
        .expect("the endpoint names the pipe");
    let pipe_path = descriptor
        .strip_prefix("pipe:")
        .expect("a pipe descriptor starts with `pipe:`");

    let client = std::fs::OpenOptions::new()
        .access_mode(READ_CONTROL)
        .open(pipe_path)
        .expect("open the pipe with READ_CONTROL");
    let sddl = dacl_sddl(client.as_raw_handle());
    drop(client);
    handle.finish().await;

    let aces = sddl
        .strip_prefix("D:P")
        .unwrap_or_else(|| panic!("the DACL is not protected: {sddl}"));
    assert!(
        aces.starts_with('(') && aces.ends_with(')') && aces.matches('(').count() == 1,
        "expected exactly one ACE: {sddl}"
    );
    let fields: Vec<&str> = aces[1..aces.len() - 1].split(';').collect();
    assert_eq!(fields.first(), Some(&"A"), "not an allow ACE: {sddl}");
    assert_eq!(
        fields.last(),
        Some(&"OW"),
        "trustee is not OWNER RIGHTS: {sddl}"
    );
}
